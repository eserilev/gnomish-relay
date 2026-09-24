// The transport of Gnomish Relay: the strip out, the slots in, `/reload`, and a
// saved-data wipe. See SPEC.md 7 and 14.2. The bridge state machine follows this model.
//
// The bounds are small, so the simulator reaches the edges: a full slot body, an
// empty slot pool, a strip that nobody reads, a wipe in the middle of a run.
module transport {

  // In the game: 30 records per body, 200 slots per UI session.
  pure val BODY_CAP = 3
  pure val POOL = 2
  pure val MSGS = 5
  pure val SHOWS = 3
  pure val MAX_WIPES = 1
  pure val CHATS = Set("a", "b", "c")

  type Key = { token: int, id: int }
  type Status = Working | Done
  type Reply = { key: Key, chat: str, status: Status }
  type Restore = { token: int, chats: Set[str] }

  // What every strip and the saved variables tell the bridge besides the messages:
  // the replies the addon has read (SPEC 7.1.1 `read`) and whether it has applied
  // the restore bundle (`restored`).
  type Report = { token: int, read: Set[int], restored: bool }

  pure val NO_RESTORE: Restore = { token: -1, chats: Set() }

  type Addon = {
    // Saved data: it survives `/reload`, and a wipe loses it.
    token: int,
    nextId: int,
    sent: int -> str,
    answered: Set[int],
    outbox: Set[int],
    chats: List[str],
    restored: bool,
    // The UI session: `/reload` loses it.
    strip: int -> int,
    slotsLeft: int,
  }

  type Bridge = {
    tokens: Set[int],
    seen: Set[Key],
    queue: List[Key],
    runs: Key -> int,
    // Every record that the addon has not read. Newest last, one per key.
    records: List[Reply],
    chats: Set[str],
    restore: Restore,
    restoreActive: bool,
    // Tokens from before a wipe. Their records leave the body once the restore is in.
    retired: Set[int],
  }

  // The saved variables file on disk: WoW writes it at `/reload`.
  type Disk = { outbox: Set[Key], report: Report }

  pure val EMPTY_DISK: Disk = { outbox: Set(), report: { token: -1, read: Set(), restored: false } }

  var addon: Addon
  var bridge: Bridge
  var body: List[Reply]
  var bundle: Restore
  var disk: Disk
  var wipes: int

  // ---------- helpers ----------

  pure def without(m: int -> int, k: int): int -> int =
    m.keys().exclude(Set(k)).mapBy(x => m.get(x))

  pure def keysOf(l: List[Reply]): Set[Key] = l.foldl(Set(), (s, r) => s.union(Set(r.key)))

  pure def recordFor(l: List[Reply], k: Key): List[Reply] = l.select(r => r.key == k)

  // Puts `r` at the newest place and drops the older record of the same key.
  pure def upsert(l: List[Reply], r: Reply): List[Reply] = l.select(x => x.key != r.key).append(r)

  pure def has(l: List[str], x: str): bool = l.foldl(false, (found, y) => found or y == x)

  pure def mergeChats(have: List[str], add: Set[str]): List[str] =
    add.fold(have, (l, c) => if (has(l, c)) l else l.append(c))

  pure def listToSet(l: List[str]): Set[str] = l.foldl(Set(), (s, x) => s.union(Set(x)))

  pure def reportOf(a: Addon): Report = { token: a.token, read: a.answered, restored: a.restored }

  // The records that go into a slot body: everything unread, except retired tokens.
  pure def live(b: Bridge): List[Reply] = b.records.select(r => not(b.retired.contains(r.key.token)))

  // A final reply leaves the body once the addon has read it. A confirmed restore
  // ends the bundle and retires every older token.
  pure def takeReport(b: Bridge, r: Report): Bridge = {
    val readKeys = r.read.map(id => { token: r.token, id: id })
    val confirmed = r.restored and b.restoreActive and b.restore.token == r.token
    {
      ...b,
      records: b.records.select(x => not(x.status == Done and readKeys.contains(x.key))),
      restoreActive: b.restoreActive and not(confirmed),
      retired: if (confirmed) b.tokens.exclude(Set(r.token)) else b.retired,
    }
  }

  // The bridge takes a message, and drops a duplicate. With a full body it refuses
  // the message and does not mark it seen, so the addon sends it again later.
  pure def admit(b: Bridge, k: Key, chat: str): Bridge =
    if (b.seen.contains(k) or live(b).length() >= BODY_CAP) b else {
      ...b,
      seen: b.seen.union(Set(k)),
      queue: b.queue.append(k),
      records: upsert(b.records, { key: k, chat: chat, status: Working }),
      chats: b.chats.union(Set(chat)),
    }

  // One publish writes the same body into every slot.
  action publish(b: Bridge): bool = all {
    body' = live(b),
    bundle' = if (b.restoreActive) b.restore else NO_RESTORE,
    bridge' = b,
  }

  // ---------- init ----------

  action init = all {
    addon' = {
      token: 1, nextId: 1, sent: Map(), answered: Set(), outbox: Set(), chats: List(),
      restored: false, strip: Map(), slotsLeft: POOL,
    },
    bridge' = {
      tokens: Set(1), seen: Set(), queue: List(), runs: Map(), records: List(),
      chats: Set(), restore: NO_RESTORE, restoreActive: false, retired: Set(),
    },
    body' = List(),
    bundle' = NO_RESTORE,
    disk' = EMPTY_DISK,
    wipes' = 0,
  }

  // ---------- the addon ----------

  // The user sends a message in a chat. The addon shows it on the strip.
  action send = {
    nondet chat = CHATS.oneOf()
    all {
      addon.nextId <= MSGS,
      addon' = {
        ...addon,
        nextId: addon.nextId + 1,
        sent: addon.sent.put(addon.nextId, chat),
        chats: mergeChats(addon.chats, Set(chat)),
        strip: addon.strip.put(addon.nextId, SHOWS),
      },
      bridge' = bridge, body' = body, bundle' = bundle, disk' = disk, wipes' = wipes,
    }
  }

  // No acknowledgment came in 40 seconds. The addon shows the strip again, and after
  // SHOWS tries it moves the message to the outbox for the reload fallback.
  action stripTimeout = {
    nondet id = addon.strip.keys().oneOf()
    val left = addon.strip.get(id)
    all {
      addon' = if (left > 1) { ...addon, strip: addon.strip.set(id, left - 1) }
               else { ...addon, strip: without(addon.strip, id), outbox: addon.outbox.union(Set(id)) },
      bridge' = bridge, body' = body, bundle' = bundle, disk' = disk, wipes' = wipes,
    }
  }

  // The addon loads its next unread slot.
  action loadSlot = all {
    addon.slotsLeft > 0,
    val mine = body.select(r => r.key.token == addon.token)
    val seenIds = keysOf(mine).map(k => k.id)
    val doneIds = keysOf(mine.select(r => r.status == Done)).map(k => k.id)
    val gotBundle = bundle.token == addon.token
    all {
      addon' = {
        ...addon,
        slotsLeft: addon.slotsLeft - 1,
        answered: addon.answered.union(doneIds),
        outbox: addon.outbox.exclude(seenIds),
        strip: addon.strip.keys().exclude(seenIds).mapBy(i => addon.strip.get(i)),
        chats: if (gotBundle) mergeChats(addon.chats, bundle.chats) else addon.chats,
        restored: addon.restored or gotBundle,
      },
      bridge' = bridge, body' = body, bundle' = bundle, disk' = disk, wipes' = wipes,
    }
  }

  // `/reload`. WoW writes the saved variables, and the UI session starts again.
  // The addon shows the strip again for every open message that is not in the outbox.
  action reload = {
    val open = addon.sent.keys().exclude(addon.answered).exclude(addon.outbox)
    all {
      addon' = { ...addon, strip: open.mapBy(_ => SHOWS), slotsLeft: POOL },
      disk' = { outbox: addon.outbox.map(id => { token: addon.token, id: id }), report: reportOf(addon) },
      bridge' = bridge, body' = body, bundle' = bundle, wipes' = wipes,
    }
  }

  // The beta client wipes the saved data. The addon makes a new token.
  action wipe = all {
    wipes < MAX_WIPES,
    addon' = {
      token: addon.token + 1, nextId: 1, sent: Map(), answered: Set(), outbox: Set(), chats: List(),
      restored: false, strip: Map(), slotsLeft: POOL,
    },
    disk' = EMPTY_DISK,
    wipes' = wipes + 1,
    bridge' = bridge, body' = body, bundle' = bundle,
  }

  // ---------- the bridge ----------

  // The bridge decodes a message strip. The report on it comes first.
  action decodeStrip = {
    nondet id = addon.strip.keys().oneOf()
    val b = takeReport(bridge, reportOf(addon))
    all {
      publish(admit(b, { token: addon.token, id: id }, addon.sent.get(id))),
      addon' = addon, disk' = disk, wipes' = wipes,
    }
  }

  // The bridge reads the outbox in the saved variables file.
  action readOutbox = {
    nondet k = disk.outbox.oneOf()
    val chat = if (k.token == addon.token and addon.sent.keys().contains(k.id)) addon.sent.get(k.id) else "a"
    val b = takeReport(bridge, disk.report)
    all { publish(admit(b, k, chat)), addon' = addon, disk' = disk, wipes' = wipes }
  }

  // A hello: a strip with no message, only the token and the report. The addon sends
  // one at login and after it applies a restore bundle. An unknown token starts the
  // restore (SPEC 7.6).
  action hello = {
    val b = takeReport(bridge, reportOf(addon))
    val known = bridge.tokens.contains(addon.token)
    all {
      publish(if (known) b else {
        ...b,
        tokens: b.tokens.union(Set(addon.token)),
        restore: { token: addon.token, chats: b.chats },
        restoreActive: true,
      }),
      addon' = addon, disk' = disk, wipes' = wipes,
    }
  }

  // The agent runs the oldest queued message, and the bridge publishes the reply.
  action runAgent = all {
    bridge.queue.length() > 0,
    val k = bridge.queue.head()
    val old = recordFor(bridge.records, k)
    val chat = if (old.length() > 0) old[0].chat else "a"
    val b = {
      ...bridge,
      queue: bridge.queue.tail(),
      runs: bridge.runs.put(k, if (bridge.runs.keys().contains(k)) bridge.runs.get(k) + 1 else 1),
      records: upsert(bridge.records, { key: k, chat: chat, status: Done }),
    }
    all { publish(b), addon' = addon, disk' = disk, wipes' = wipes },
  }

  action step = any {
    send,
    stripTimeout,
    loadSlot,
    reload,
    wipe,
    decodeStrip,
    readOutbox,
    hello,
    runAgent,
  }

  // ---------- the four properties of SPEC 14.2 ----------

  // 1. The agent never runs one message twice.
  val runsOnce = bridge.runs.keys().forall(k => bridge.runs.get(k) <= 1)

  // 2. A publish never loses a reply that the addon has not read.
  val noLostReply = bridge.records.select(r => r.key.token == addon.token and r.status == Done)
    .foldl(true, (ok, r) => ok and (addon.answered.contains(r.key.id) or keysOf(body).contains(r.key)))

  // 3. After a saved-data wipe, the restore never duplicates or drops a chat.
  val noDuplicateChat = addon.chats.length() == listToSet(addon.chats).size()
  val restoreComplete =
    (wipes > 0 and bridge.restore.token == addon.token and not(bridge.restoreActive))
    implies bridge.restore.chats.subseteq(listToSet(addon.chats))
  val restoreSafe = noDuplicateChat and restoreComplete

  // 4. Each sent message ends with a reply or an error, also across `/reload`: an open
  // message is always somewhere that still leads to a reply.
  val noStuckMessage = addon.sent.keys().forall(id => {
    val k = { token: addon.token, id: id }
    or {
      addon.answered.contains(id),
      addon.strip.keys().contains(id),
      addon.outbox.contains(id),
      bridge.queue.foldl(false, (found, q) => found or q == k),
      keysOf(body).contains(k),
    }
  })

  // The fixes keep S12: a body never holds more than BODY_CAP records.
  val bodyBounded = body.length() <= BODY_CAP

  val allProperties = runsOnce and noLostReply and restoreSafe and noStuckMessage and bodyBounded

  // ---------- witnesses ----------
  // Each one must fail: it names a state that the simulator must reach. If a witness
  // holds, the properties above pass only because the hard states never happen.

  // The body is full, so the bridge refuses new messages.
  val neverFull = body.length() < BODY_CAP
  // A wipe happened, and the addon confirmed the restore.
  val neverRestored = not(wipes > 0 and bridge.restore.token == addon.token and not(bridge.restoreActive))
  // The addon read two replies.
  val neverTwoAnswers = addon.answered.size() < 2
  // A message went through the outbox and got its reply.
  val neverOutboxReply = not(disk.outbox.exists(k => k.token == addon.token and addon.answered.contains(k.id)))
}
