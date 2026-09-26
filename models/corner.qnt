// The shared strip corner: two addons draw their strip in the same corner of the
// screen and take turns through one shared value. See SPEC.md 7.1 and 9.7, decision 13.
// `addon/transport/Strip.lua` follows this model.
//
// One time step is one second. Each addon runs its Tick once per second, in any order,
// and a send can show a strip at any moment. A screenshot event comes back within one
// second, or never, and every addon sees it. A hostile addon can write the shared value
// at any moment, but it draws no strip.
//
// The times are small, so the simulator reaches the edges. They keep the order of the
// real times: event < tail, shot timeout < hold, wait bound < blocked line, and
// shot timeout + retry + blocked line < fresh.
module corner {

  pure val ADDONS = Set("relay", "timeways")
  pure val NOBODY = ""
  pure val HOSTILE = "hostile"
  pure val PLAYER = "player"

  // In the game: 1 s, 2 s, 10 s, 12 s, 3 s, 40 s, 3 shows, 30 s, and 270 s. So an
  // honest wait is at most 15 s.
  pure val EVENT_MAX = 1
  pure val TAIL = 2
  pure val SHOT_TIMEOUT = 3
  pure val HOLD = 4
  pure val WAIT_FRESH = 2
  pure val RETRY = 3
  pure val SHOWS = 2
  pure val BLOCK_AFTER = 9
  pure val FRESH = 17
  // With no hostile addon, a waiting addon gets the corner within this time: the tail
  // of its own last strip, one strip and tail of the other addon, and one Tick.
  pure val WAIT_BOUND = TAIL + SHOT_TIMEOUT + TAIL + 1
  pure val MAX_SENDS = 3

  // Idle: no strip. Waiting: a strip is due, and the corner is not free.
  // Drawn: the strip shows, the screenshot comes 0.1 s later. Shooting: Screenshot() ran.
  type Phase = Idle | Waiting | Drawn | Shooting

  type Addon = {
    phase: Phase,
    // A message is open: the bridge has not acknowledged it.
    open: bool,
    shows: int,
    // Seconds until the next show. The retry timer: it runs only while the addon is idle.
    retryIn: int,
    // Seconds since the addon signed the frame of the message. `Messages.lua` signs it
    // again for each strip.
    age: int,
    drawnAt: int,
    // Seconds of waiting without a break.
    waited: int,
    showsAtWait: int,
    warned: bool,
    outbox: bool,
    ticked: bool,
    sends: int,
  }

  // The wait mark of an addon: when it started to wait, and when it last asked.
  type Wait = { since: int, at: int }

  // The shared value. `waits` holds the wait mark of each addon that waits for the corner.
  type Corner = { holder: str, endsAt: int, waits: str -> Wait }

  // `owner` is only for the checks. The addons never see it.
  type Event = { owner: str, at: int }

  var now: int
  var addons: str -> Addon
  var corner: Corner
  var events: Set[Event]
  var hostile: bool
  // The bridge runs and acknowledges strips. Without it, messages go to the outbox.
  var bridge: bool
  // History for the checks. Quint reads `x' = a or b` as `(x' = a) or b`, so each
  // update of these has parentheses.
  var wrongEnd: bool
  var tookAfterWait: bool

  pure val IDLE: Addon = {
    phase: Idle, open: false, shows: 0, retryIn: 0, age: 0, drawnAt: 0, waited: 0,
    showsAtWait: 0, warned: false, outbox: false, ticked: false, sends: 0,
  }

  pure val FREE: Corner = { holder: NOBODY, endsAt: 0, waits: Map() }

  // ---------- the corner rules ----------

  pure def heldByOther(c: Corner, me: str, t: int): bool =
    c.holder != NOBODY and c.holder != me and c.endsAt > t

  // A waiter asks again every second, so an old mark belongs to an addon that stopped.
  pure def waits(c: Corner, who: str, t: int): bool =
    c.waits.keys().contains(who) and t - c.waits.get(who).at < WAIT_FRESH

  // The turn rule: an addon that waits longer goes first.
  pure def waitsLonger(c: Corner, other: str, me: str, t: int): bool =
    other != me and waits(c, other, t) and (not(waits(c, me, t)) or c.waits.get(other).since < c.waits.get(me).since)

  pure def canTake(c: Corner, me: str, t: int): bool =
    not(heldByOther(c, me, t)) and not(c.waits.keys().exists(other => waitsLonger(c, other, me, t)))

  pure def take(c: Corner, me: str, t: int): Corner =
    { holder: me, endsAt: t + HOLD, waits: c.waits.keys().exclude(Set(me)).mapBy(w => c.waits.get(w)) }

  pure def markWait(c: Corner, me: str, t: int): Corner = {
    val since = if (waits(c, me, t)) c.waits.get(me).since else t
    { ...c, waits: c.waits.put(me, { since: since, at: t }) }
  }

  // The holder keeps the corner for a short tail after its strip ends. The event of its
  // own shot can still come, and it then finds no strip of another addon to end.
  pure def release(c: Corner, me: str, t: int): Corner =
    if (c.holder == me) { ...c, endsAt: t + TAIL } else c

  pure def showsStrip(a: Addon): bool = a.phase == Drawn or a.phase == Shooting

  pure def wantsStrip(a: Addon): bool =
    a.open and not(a.outbox) and a.retryIn == 0 and (a.phase == Idle or a.phase == Waiting)

  // ---------- init ----------

  action init = all {
    now' = 0,
    addons' = ADDONS.mapBy(_ => IDLE),
    corner' = FREE,
    events' = Set(),
    nondet h = Set(false, true).oneOf()
    hostile' = h,
    nondet up = Set(false, true).oneOf()
    bridge' = up,
    wrongEnd' = false,
    tookAfterWait' = false,
  }

  // ---------- one addon ----------

  // `Strip.Show`: take the corner and draw, or wait. After SHOWS shows with no
  // acknowledgment, the message goes to the outbox (SPEC 7.5).
  pure def attempt(a: Addon, c: Corner, me: str, t: int): (Addon, Corner) =
    if (not(wantsStrip(a))) (a, c)
    else if (a.shows >= SHOWS) ({ ...a, phase: Idle, outbox: true }, c)
    else if (canTake(c, me, t))
      ({ ...a, phase: Drawn, drawnAt: t, age: 0, waited: 0, warned: false }, take(c, me, t))
    else {
      val start = a.phase != Waiting
      ({
        ...a,
        phase: Waiting,
        showsAtWait: if (start) a.shows else a.showsAtWait,
        warned: a.warned or a.waited >= BLOCK_AFTER,
      }, markWait(c, me, t))
    }

  pure def tookAfterWaiting(before: Addon, after: Addon): bool =
    before.phase == Waiting and after.phase == Drawn

  action tryShow(me: str, markTick: bool): bool = {
    val a = addons.get(me)
    val r = attempt(a, corner, me, now)
    all {
      not(markTick) or not(a.ticked),
      addons' = addons.set(me, { ...r._1, ticked: a.ticked or markTick }),
      corner' = r._2,
      tookAfterWait' = (tookAfterWait or tookAfterWaiting(a, r._1)),
      now' = now, events' = events, hostile' = hostile, bridge' = bridge, wrongEnd' = wrongEnd,
    }
  }

  // The Tick of the addon, once per second.
  action tick = { nondet me = ADDONS.oneOf() tryShow(me, true) }

  // The user sends a message. `Messages.Queue` shows the strip at once.
  action send = {
    nondet me = ADDONS.oneOf()
    val a = addons.get(me)
    all {
      not(a.open),
      a.sends < MAX_SENDS,
      val fresh = { ...a, open: true, shows: 0, retryIn: 0, age: 0, outbox: false, sends: a.sends + 1 }
      val r = attempt(fresh, corner, me, now)
      all {
        addons' = addons.set(me, r._1),
        corner' = r._2,
        tookAfterWait' = tookAfterWait,
      },
      now' = now, events' = events, hostile' = hostile, bridge' = bridge, wrongEnd' = wrongEnd,
    }
  }

  // The timer 0.1 s after the draw calls Screenshot().
  action shoot = {
    nondet me = ADDONS.oneOf()
    val a = addons.get(me)
    all {
      a.phase == Drawn,
      addons' = addons.set(me, { ...a, phase: Shooting }),
      events' = events.union(Set({ owner: me, at: now })),
      now' = now, corner' = corner, hostile' = hostile, bridge' = bridge, wrongEnd' = wrongEnd, tookAfterWait' = tookAfterWait,
    }
  }

  // The player takes a screenshot. Its event looks like ours. One at a time keeps the
  // traces short.
  action playerShot = all {
    not(events.exists(e => e.owner == PLAYER)),
    events' = events.union(Set({ owner: PLAYER, at: now })),
    now' = now, addons' = addons, corner' = corner, hostile' = hostile, bridge' = bridge, wrongEnd' = wrongEnd,
    tookAfterWait' = tookAfterWait,
  }

  // An event ends a strip only for the addon that holds the corner and has called
  // Screenshot(). A good shot counts as a show, and the retry timer starts again.
  pure def onEvent(a: Addon, c: Corner, me: str): bool =
    a.phase == Shooting and c.holder == me

  action deliver = {
    nondet e = events.oneOf()
    val enders = ADDONS.filter(me => onEvent(addons.get(me), corner, me))
    all {
      addons' = ADDONS.mapBy(me => {
        val a = addons.get(me)
        if (enders.contains(me)) { ...a, phase: Idle, shows: a.shows + 1, retryIn: RETRY } else a
      }),
      corner' = enders.fold(corner, (c, me) => release(c, me, now)),
      events' = events.exclude(Set(e)),
      wrongEnd' = (wrongEnd or enders.exists(me => e.owner != me and e.owner != PLAYER)),
      now' = now, hostile' = hostile, bridge' = bridge, tookAfterWait' = tookAfterWait,
    }
  }

  // The client never reports a shot, so the strip ends at its timeout.
  action loseEvent = {
    nondet e = events.oneOf()
    all {
      events' = events.exclude(Set(e)),
      now' = now, addons' = addons, corner' = corner, hostile' = hostile, bridge' = bridge, wrongEnd' = wrongEnd,
      tookAfterWait' = tookAfterWait,
    }
  }

  // A hostile addon writes the shared value. It holds the corner and can fake a wait mark.
  action hostileWrite = all {
    hostile,
    corner.holder != HOSTILE,
    nondet fakeWaiter = Set(false, true).oneOf()
    corner' = {
      holder: HOSTILE,
      endsAt: now + 100,
      waits: if (fakeWaiter) corner.waits.put(HOSTILE, { since: -100, at: now }) else corner.waits,
    },
    now' = now, addons' = addons, events' = events, hostile' = hostile, bridge' = bridge, wrongEnd' = wrongEnd,
    tookAfterWait' = tookAfterWait,
  }

  // The bridge acknowledges the message of an addon.
  action ack = {
    nondet me = ADDONS.oneOf()
    val a = addons.get(me)
    all {
      bridge,
      a.open,
      a.shows > 0,
      not(showsStrip(a)),
      addons' = addons.set(me, { ...a, open: false, phase: Idle, outbox: false }),
      now' = now, corner' = corner, events' = events, hostile' = hostile, bridge' = bridge, wrongEnd' = wrongEnd,
      tookAfterWait' = tookAfterWait,
    }
  }

  // One second passes. Each addon has run its Tick, no strip waits for its 0.1 s timer,
  // and no event is late. A strip with no event ends at its timeout.
  pure def timesOut(a: Addon, t: int): bool = a.phase == Shooting and t + 1 - a.drawnAt >= SHOT_TIMEOUT

  pure def second(a: Addon, t: int): Addon = {
    val timedOut = timesOut(a, t)
    {
      ...a,
      phase: if (timedOut) Idle else a.phase,
      retryIn: if (a.phase == Idle and a.open and a.retryIn > 0) a.retryIn - 1 else a.retryIn,
      age: if (a.open) a.age + 1 else a.age,
      waited: if (a.phase == Waiting) a.waited + 1 else 0,
      ticked: false,
    }
  }

  action advance = all {
    ADDONS.forall(me => addons.get(me).ticked and addons.get(me).phase != Drawn),
    events.forall(e => now + 1 - e.at <= EVENT_MAX),
    now' = now + 1,
    addons' = ADDONS.mapBy(me => second(addons.get(me), now)),
    corner' = ADDONS.filter(me => timesOut(addons.get(me), now)).fold(corner, (c, me) => release(c, me, now + 1)),
    events' = events, hostile' = hostile, bridge' = bridge, wrongEnd' = wrongEnd, tookAfterWait' = tookAfterWait,
  }

  action step = any {
    tick,
    send,
    shoot,
    playerShot,
    deliver,
    loseEvent,
    hostileWrite,
    ack,
    advance,
  }

  // ---------- the properties of SPEC 14.2 ----------

  // 1. Two addons never show a strip at the same time.
  val oneStrip = ADDONS.forall(x => ADDONS.forall(y => x == y or not(showsStrip(addons.get(x)) and showsStrip(addons.get(y)))))

  // 2. A screenshot event ends only the strip of the addon that holds the corner:
  // never a strip whose own screenshot the event does not belong to.
  val ownEvents = not(wrongEnd)

  // 3. The retry timer does not run while the addon waits, and no show counts.
  // So the outbox comes only after SHOWS real shows.
  val retryPaused = ADDONS.forall(me => {
    val a = addons.get(me)
    and {
      a.phase == Waiting implies (a.retryIn == 0 and a.shows == a.showsAtWait),
      a.outbox implies a.shows >= SHOWS,
    }
  })

  // 4. An addon that cannot get the corner shows the blocked line before its frame is stale.
  val blockedInTime = ADDONS.forall(me => {
    val a = addons.get(me)
    (a.phase == Waiting and (a.age >= FRESH or a.waited > BLOCK_AFTER)) implies a.warned
  })

  // 5. With no hostile addon, a waiting addon gets the corner within WAIT_BOUND, and no
  // addon shows the blocked line.
  val fairWait = not(hostile) implies ADDONS.forall(me => {
    val a = addons.get(me)
    a.waited <= WAIT_BOUND and not(a.warned)
  })

  val allProperties = oneStrip and ownEvents and retryPaused and blockedInTime and fairWait

  // ---------- witnesses ----------
  // Each one must fail: it names a state that the simulator must reach.

  // An addon waited for the corner, then got it.
  val neverTurn = not(tookAfterWait)
  // An addon showed the blocked line.
  val neverWarned = ADDONS.forall(me => not(addons.get(me).warned))
  // An addon moved its message to the outbox.
  val neverOutbox = ADDONS.forall(me => not(addons.get(me).outbox))
  // Both addons got a strip out.
  val neverBoth = not(ADDONS.forall(me => addons.get(me).shows > 0))
  // A retry went out.
  val neverRetry = ADDONS.forall(me => addons.get(me).shows < 2)
}
