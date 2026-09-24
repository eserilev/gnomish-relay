// The transport of Gnomish Relay: the strip out, the slots in, `/reload`, and a
// saved-data wipe. See SPEC.md 7 and 14.2. The bridge state machine follows this model.
//
// The bounds are small, so the simulator reaches the edges: a full slot body, an
// empty slot pool, a strip that nobody reads, a wipe in the middle of a run.
module transport {

  // In the game: 30 replies per body, 200 slots per UI session.
  pure val BODY_CAP = 3
  pure val POOL = 2
  pure val MSGS = 5
  pure val SHOWS = 3
  pure val RESTORE_PUBLISHES = 3
  pure val MAX_WIPES = 1
  pure val CHATS = Set("a", "b", "c")

  type Key = { token: int, id: int }
  type Status = Working | Done
  type Reply = { key: Key, chat: str, status: Status }
  type Restore = { token: int, chats: Set[str] }

  pure val NO_RESTORE: Restore = { token: -1, chats: Set() }

  type Addon = {
    // Saved data: it survives `/reload`, and a wipe loses it.
    token: int,
    nextId: int,
    sent: int -> str,
    answered: Set[int],
    outbox: Set[int],
    chats: List[str],
    // The UI session: `/reload` loses it.
    strip: int -> int,
    slotsLeft: int,
  }

  type Bridge = {
    tokens: Set[int],
    seen: Set[Key],
    queue: List[Key],
    runs: Key -> int,
    // Newest last, one record per key.
    records: List[Reply],
    chats: Set[str],
    restore: Restore,
    restoreLeft: int,
  }

  // The addon, the bridge, the slot files, the saved variables file on disk.
  var addon: Addon
  var bridge: Bridge
  var body: List[Reply]
  var bundle: Restore
  var disk: Set[Key]
  var wipes: int

  // ---------- helpers ----------

  pure def without(m: int -> int, k: int): int -> int =
    m.keys().exclude(Set(k)).mapBy(x => m.get(x))

  pure def keysOf(l: List[Reply]): Set[Key] = l.foldl(Set(), (s, r) => s.union(Set(r.key)))

  pure def recordFor(l: List[Reply], k: Key): List[Reply] = l.select(r => r.key == k)

  // Puts `r` at the newest place and drops the older record of the same key.
  pure def upsert(l: List[Reply], r: Reply): List[Reply] = l.select(x => x.key != r.key).append(r)

  pure def lastN(l: List[Reply], n: int): List[Reply] =
    if (l.length() <= n) l else l.slice(l.length() - n, l.length())

  // What one publish writes into every slot: the last BODY_CAP records (SPEC 7.3).
  pure def bodyOf(b: Bridge): List[Reply] = lastN(b.records, BODY_CAP)

  pure def has(l: List[str], x: str): bool = l.foldl(false, (found, y) => found or y == x)

  pure def mergeChats(have: List[str], add: Set[str]): List[str] =
    add.fold(have, (l, c) => if (has(l, c)) l else l.append(c))

  pure def listToSet(l: List[str]): Set[str] = l.foldl(Set(), (s, x) => s.union(Set(x)))

  // Publishes the records, and the restore bundle while it lasts.
  action publish(b: Bridge): bool = all {
    body' = bodyOf(b),
    bundle' = if (b.restoreLeft > 0) b.restore else NO_RESTORE,
    bridge' = if (b.restoreLeft > 0) { ...b, restoreLeft: b.restoreLeft - 1 } else b,
  }

  // The bridge takes a message from the strip or the outbox, and drops a duplicate.
  pure def admit(b: Bridge, k: Key, chat: str): Bridge =
    if (b.seen.contains(k)) b else {
      ...b,
      seen: b.seen.union(Set(k)),
      queue: b.queue.append(k),
      records: upsert(b.records, { key: k, chat: chat, status: Working }),
      chats: b.chats.union(Set(chat)),
    }

  // ---------- init ----------

  action init = all {
    addon' = {
      token: 1, nextId: 1, sent: Map(), answered: Set(), outbox: Set(), chats: List(),
      strip: Map(), slotsLeft: POOL,
    },
    bridge' = {
      tokens: Set(1), seen: Set(), queue: List(), runs: Map(), records: List(),
      chats: Set(), restore: NO_RESTORE, restoreLeft: 0,
    },
    body' = List(),
    bundle' = NO_RESTORE,
    disk' = Set(),
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
    val chats1 = if (bundle.token == addon.token) mergeChats(addon.chats, bundle.chats) else addon.chats
    all {
      addon' = {
        ...addon,
        slotsLeft: addon.slotsLeft - 1,
        answered: addon.answered.union(doneIds),
        outbox: addon.outbox.exclude(seenIds),
        strip: addon.strip.keys().exclude(seenIds).mapBy(i => addon.strip.get(i)),
        chats: chats1,
      },
      bridge' = bridge, body' = body, bundle' = bundle, disk' = disk, wipes' = wipes,
    }
  }

  // `/reload`. WoW writes the saved variables, and the UI session starts again.
  // The model requires that the addon shows the strip again for every open message
  // that is not in the outbox. SPEC 7.5 does not say this yet.
  action reload = {
    val open = addon.sent.keys().exclude(addon.answered).exclude(addon.outbox)
    all {
      addon' = { ...addon, strip: open.mapBy(_ => SHOWS), slotsLeft: POOL },
      disk' = addon.outbox.map(id => { token: addon.token, id: id }),
      bridge' = bridge, body' = body, bundle' = bundle, wipes' = wipes,
    }
  }

  // The beta client wipes the saved data. The addon makes a new token and sends a hello.
  action wipe = all {
    wipes < MAX_WIPES,
    addon' = {
      token: addon.token + 1, nextId: 1, sent: Map(), answered: Set(), outbox: Set(), chats: List(),
      strip: Map(), slotsLeft: POOL,
    },
    disk' = Set(),
    wipes' = wipes + 1,
    bridge' = bridge, body' = body, bundle' = bundle,
  }

  // ---------- the bridge ----------

  // The bridge decodes a strip.
  action decodeStrip = {
    nondet id = addon.strip.keys().oneOf()
    all {
      publish(admit(bridge, { token: addon.token, id: id }, addon.sent.get(id))),
      addon' = addon, disk' = disk, wipes' = wipes,
    }
  }

  // The bridge reads the outbox in the saved variables file.
  action readOutbox = {
    nondet k = disk.oneOf()
    val chat = if (k.token == addon.token and addon.sent.keys().contains(k.id)) addon.sent.get(k.id) else "a"
    all { publish(admit(bridge, k, chat)), addon' = addon, disk' = disk, wipes' = wipes }
  }

  // The addon says hello with its token. An unknown token starts the restore (SPEC 7.6).
  action hello = all {
    not(bridge.tokens.contains(addon.token)),
    val b = {
      ...bridge,
      tokens: bridge.tokens.union(Set(addon.token)),
      restore: { token: addon.token, chats: bridge.chats },
      restoreLeft: RESTORE_PUBLISHES,
    }
    all { publish(b), addon' = addon, disk' = disk, wipes' = wipes },
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
    (wipes > 0 and bridge.restore.token == addon.token and bridge.restoreLeft == 0 and
      addon.slotsLeft < POOL)
    implies bridge.chats.subseteq(listToSet(addon.chats))
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

  val allProperties = runsOnce and noLostReply and restoreSafe and noStuckMessage
}
