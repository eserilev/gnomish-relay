// The link between the game half and the server half (SPEC.md 11.7.5 and 11.7.6).
//
// The link drops and copies messages, either half restarts, and each reconnect starts the
// numbers again at 1. The bounds are small, so the simulator reaches these edges.
module remoteLink {

  // True for the old version that acks before `state.json` is on disk. It must fail.
  const ACK_EARLY: bool

  pure val MSGS = Set(1, 2, 3)
  pure val REQUESTS = Set(1, 2)
  pure val MAX_CONNECTS = 3
  pure val MAX_RESTARTS = 2

  type Input = { n: int, msg: int }
  type ToServer = { conn: int, n: int, msg: int }
  type Ack = { conn: int, ack: int }
  type Verdict = { conn: int, request: int }

  type Game = {
    outbox: List[Input],
    next: int,
    // The copies of the server requests in the request folder of the game computer.
    copies: Set[int],
  }

  // `seen`, `queue`, and `started` are in `state.json`. The `pending` parts and `lastN`
  // live in memory until the next store.
  type Server = {
    seen: Set[int],
    queue: List[int],
    started: Set[int],
    pendingSeen: Set[int],
    pendingQueue: List[int],
    lastN: int,
    acked: int,
    open: Set[int],
    answered: Set[int],
  }

  var game: Game
  var server: Server
  var conn: int
  var toServer: Set[ToServer]
  var acks: Set[Ack]
  var verdicts: Set[Verdict]
  var runs: int -> int
  // Ghost state for the properties.
  var ackedMsgs: Set[int]
  var lateAnswer: bool
  var ignoredVerdict: bool
  var connects: int
  var restarts: int
  var sentTwice: bool
  var reconnectedWithOutbox: bool

  // ---------- helpers ----------

  pure def dropAcked(l: List[Input], ack: int): List[Input] = l.select(i => i.n > ack)

  pure def renumber(l: List[Input]): List[Input] =
    l.indices().fold(List(), (out, i) => out.append({ n: i + 1, msg: l[i].msg }))

  pure def has(l: List[int], x: int): bool = l.foldl(false, (found, y) => found or y == x)

  pure def msgsUpTo(l: List[Input], ack: int): Set[int] =
    l.foldl(Set(), (s, i) => if (i.n <= ack) s.union(Set(i.msg)) else s)

  // ---------- init ----------

  action init = all {
    game' = { outbox: List(), next: 1, copies: Set() },
    server' = {
      seen: Set(), queue: List(), started: Set(), pendingSeen: Set(), pendingQueue: List(),
      lastN: 0, acked: 0, open: Set(), answered: Set(),
    },
    conn' = 1,
    toServer' = Set(),
    acks' = Set(),
    verdicts' = Set(),
    runs' = MSGS.mapBy(_ => 0),
    ackedMsgs' = Set(),
    lateAnswer' = false,
    ignoredVerdict' = false,
    connects' = 1,
    restarts' = 0,
    sentTwice' = false,
    reconnectedWithOutbox' = false,
  }

  // ---------- the game half ----------

  // A strip arrives. The player can send the same message again only through a copy on
  // the link, so each message enters the outbox once.
  action readStrip = {
    nondet m = MSGS.oneOf()
    all {
      not(game.outbox.foldl(false, (f, i) => f or i.msg == m)),
      not(server.seen.contains(m)),
      not(server.pendingSeen.contains(m)),
      game' = { ...game, outbox: game.outbox.append({ n: game.next, msg: m }), next: game.next + 1 },
      server' = server, conn' = conn, toServer' = toServer, acks' = acks, verdicts' = verdicts,
      runs' = runs, ackedMsgs' = ackedMsgs, lateAnswer' = lateAnswer, ignoredVerdict' = ignoredVerdict, connects' = connects,
      restarts' = restarts, sentTwice' = sentTwice, reconnectedWithOutbox' = reconnectedWithOutbox,
    }
  }

  action sendOutbox = all {
    game.outbox.length() > 0,
    toServer' = toServer.union(game.outbox.foldl(Set(), (s, i) => s.union(Set({ conn: conn, n: i.n, msg: i.msg })))),
    game' = game, server' = server, conn' = conn, acks' = acks, verdicts' = verdicts,
    runs' = runs, ackedMsgs' = ackedMsgs, lateAnswer' = lateAnswer, ignoredVerdict' = ignoredVerdict, connects' = connects,
    restarts' = restarts, sentTwice' = sentTwice, reconnectedWithOutbox' = reconnectedWithOutbox,
  }

  action takeAck = {
    nondet a = acks.oneOf()
    all {
      a.conn == conn,
      game' = { ...game, outbox: dropAcked(game.outbox, a.ack) },
      acks' = acks.exclude(Set(a)),
      server' = server, conn' = conn, toServer' = toServer, verdicts' = verdicts,
      runs' = runs, ackedMsgs' = ackedMsgs, lateAnswer' = lateAnswer, ignoredVerdict' = ignoredVerdict, connects' = connects,
      restarts' = restarts, sentTwice' = sentTwice, reconnectedWithOutbox' = reconnectedWithOutbox,
    }
  }

  // The dialog or `gnomish-relay approve` answers a copy.
  action answerCopy = {
    nondet r = REQUESTS.oneOf()
    all {
      game.copies.contains(r),
      verdicts' = verdicts.union(Set({ conn: conn, request: r })),
      game' = game, server' = server, conn' = conn, toServer' = toServer, acks' = acks,
      runs' = runs, ackedMsgs' = ackedMsgs, lateAnswer' = lateAnswer, ignoredVerdict' = ignoredVerdict, connects' = connects,
      restarts' = restarts, sentTwice' = sentTwice, reconnectedWithOutbox' = reconnectedWithOutbox,
    }
  }

  // The link drops. Both halves start a new connection, and its numbers start at 1. After
  // the hello, the server sends every waiting request, so the game drops other copies.
  action reconnect = all {
    connects < MAX_CONNECTS,
    game' = {
      outbox: renumber(game.outbox),
      next: game.outbox.length() + 1,
      copies: game.copies.intersect(server.open),
    },
    server' = { ...server, lastN: 0, acked: 0 },
    conn' = conn + 1,
    connects' = connects + 1,
    reconnectedWithOutbox' = (reconnectedWithOutbox or game.outbox.length() > 0),
    toServer' = toServer, acks' = acks, verdicts' = verdicts,
    runs' = runs, ackedMsgs' = ackedMsgs, lateAnswer' = lateAnswer, ignoredVerdict' = ignoredVerdict,
    restarts' = restarts, sentTwice' = sentTwice,
  }

  // ---------- the server half ----------

  // The relay lane applies the records. It takes the numbers of a connection in order:
  // it skips a number it has taken, and a gap waits. The replay store drops a message it
  // has seen, also one that comes again on a new connection.
  action applyInput = {
    nondet t = toServer.oneOf()
    val dup = server.seen.contains(t.msg) or server.pendingSeen.contains(t.msg)
    val s = if (dup) server else {
      ...server,
      pendingSeen: server.pendingSeen.union(Set(t.msg)),
      pendingQueue: server.pendingQueue.append(t.msg),
    }
    all {
      t.conn == conn,
      t.n == server.lastN + 1,
      server' = { ...s, lastN: t.n, acked: if (ACK_EARLY) t.n else s.acked },
      acks' = if (ACK_EARLY) acks.union(Set({ conn: conn, ack: t.n })) else acks,
      ackedMsgs' = if (ACK_EARLY) ackedMsgs.union(msgsUpTo(game.outbox, t.n)) else ackedMsgs,
      sentTwice' = (sentTwice or dup),
      game' = game, conn' = conn, toServer' = toServer, verdicts' = verdicts,
      runs' = runs, lateAnswer' = lateAnswer, ignoredVerdict' = ignoredVerdict, connects' = connects,
      restarts' = restarts, reconnectedWithOutbox' = reconnectedWithOutbox,
    }
  }

  // `state.json` is on disk. Only then the ack goes out.
  action store = {
    val s = {
      ...server,
      seen: server.seen.union(server.pendingSeen),
      queue: server.queue.concat(server.pendingQueue),
      pendingSeen: Set(),
      pendingQueue: List(),
      acked: server.lastN,
    }
    all {
      server' = s,
      acks' = if (server.lastN > 0) acks.union(Set({ conn: conn, ack: server.lastN })) else acks,
      ackedMsgs' = ackedMsgs.union(msgsUpTo(game.outbox, server.lastN)),
      game' = game, conn' = conn, toServer' = toServer, verdicts' = verdicts,
      runs' = runs, lateAnswer' = lateAnswer, ignoredVerdict' = ignoredVerdict, connects' = connects,
      restarts' = restarts, sentTwice' = sentTwice, reconnectedWithOutbox' = reconnectedWithOutbox,
    }
  }

  // A run starts only for a message whose seen mark is on disk, and marks it started.
  action startRun = all {
    server.queue.length() > 0,
    val m = server.queue.head()
    all {
      server' = { ...server, queue: server.queue.tail(), started: server.started.union(Set(m)) },
      runs' = runs.set(m, runs.get(m) + 1),
    },
    game' = game, conn' = conn, toServer' = toServer, acks' = acks, verdicts' = verdicts,
    ackedMsgs' = ackedMsgs, lateAnswer' = lateAnswer, ignoredVerdict' = ignoredVerdict, connects' = connects,
    restarts' = restarts, sentTwice' = sentTwice, reconnectedWithOutbox' = reconnectedWithOutbox,
  }

  // A restart loses what is in memory, and the link with it.
  action restartServer = all {
    restarts < MAX_RESTARTS,
    connects < MAX_CONNECTS,
    server' = { ...server, pendingSeen: Set(), pendingQueue: List(), lastN: 0, acked: 0 },
    conn' = conn + 1,
    connects' = connects + 1,
    restarts' = restarts + 1,
    game' = { outbox: renumber(game.outbox), next: game.outbox.length() + 1, copies: game.copies.intersect(server.open) },
    toServer' = toServer, acks' = acks, verdicts' = verdicts, runs' = runs,
    ackedMsgs' = ackedMsgs, lateAnswer' = lateAnswer, ignoredVerdict' = ignoredVerdict, sentTwice' = sentTwice,
    reconnectedWithOutbox' = (reconnectedWithOutbox or game.outbox.length() > 0),
  }

  action openRequest = {
    nondet r = REQUESTS.oneOf()
    all {
      not(server.open.contains(r)),
      not(server.answered.contains(r)),
      server' = { ...server, open: server.open.union(Set(r)) },
      game' = game, conn' = conn, toServer' = toServer, acks' = acks, verdicts' = verdicts,
      runs' = runs, ackedMsgs' = ackedMsgs, lateAnswer' = lateAnswer, ignoredVerdict' = ignoredVerdict, connects' = connects,
      restarts' = restarts, sentTwice' = sentTwice, reconnectedWithOutbox' = reconnectedWithOutbox,
    }
  }

  // The `request` message reaches the game, which writes its copy.
  action copyRequest = {
    nondet r = REQUESTS.oneOf()
    all {
      server.open.contains(r),
      game' = { ...game, copies: game.copies.union(Set(r)) },
      server' = server, conn' = conn, toServer' = toServer, acks' = acks, verdicts' = verdicts,
      runs' = runs, ackedMsgs' = ackedMsgs, lateAnswer' = lateAnswer, ignoredVerdict' = ignoredVerdict, connects' = connects,
      restarts' = restarts, sentTwice' = sentTwice, reconnectedWithOutbox' = reconnectedWithOutbox,
    }
  }

  // The timeout, Stop, or an answer on the server. The `closed` message removes the copy.
  action closeRequest = {
    nondet r = REQUESTS.oneOf()
    all {
      server.open.contains(r),
      server' = { ...server, open: server.open.exclude(Set(r)) },
      game' = { ...game, copies: game.copies.exclude(Set(r)) },
      conn' = conn, toServer' = toServer, acks' = acks, verdicts' = verdicts,
      runs' = runs, ackedMsgs' = ackedMsgs, lateAnswer' = lateAnswer, ignoredVerdict' = ignoredVerdict, connects' = connects,
      restarts' = restarts, sentTwice' = sentTwice, reconnectedWithOutbox' = reconnectedWithOutbox,
    }
  }

  // The answer file is written with `create_new` only while the request waits.
  action takeVerdict = {
    nondet v = verdicts.oneOf()
    val waits = server.open.contains(v.request) and not(server.answered.contains(v.request))
    all {
      server' = if (waits) { ...server, answered: server.answered.union(Set(v.request)), open: server.open.exclude(Set(v.request)) } else server,
      lateAnswer' = (lateAnswer or (waits and server.answered.contains(v.request))),
      ignoredVerdict' = (ignoredVerdict or not(waits)),
      verdicts' = verdicts.exclude(Set(v)),
      game' = game, conn' = conn, toServer' = toServer, acks' = acks,
      runs' = runs, ackedMsgs' = ackedMsgs, connects' = connects,
      restarts' = restarts, sentTwice' = sentTwice, reconnectedWithOutbox' = reconnectedWithOutbox,
    }
  }

  action step = any {
    readStrip, sendOutbox, takeAck, answerCopy, reconnect,
    applyInput, store, startRun, restartServer,
    openRequest, copyRequest, closeRequest, takeVerdict,
  }

  // ---------- properties ----------

  // L1: a message runs at most once.
  val runsOnce = MSGS.forall(m => runs.get(m) <= 1)

  // L2: an acked message is in the state on disk of the server.
  val ackedIsStored = ackedMsgs.forall(m => server.seen.contains(m))

  // L3: an answer applies only while its request waits, so a request gets one answer.
  val answerOnlyWhileOpen = not(lateAnswer) and server.answered.intersect(server.open).size() == 0

  // ---------- witnesses: each must fail ----------

  val neverSentTwice = not(sentTwice)
  val neverReconnectWithOutbox = not(reconnectedWithOutbox)
  val neverIgnoredVerdict = not(ignoredVerdict)
  val neverRanAfterRestart = not(restarts > 0 and MSGS.exists(m => runs.get(m) > 0))
}

module remote {
  import remoteLink(ACK_EARLY = false).*
}

// The old version: an ack before `state.json` is on disk. `ackedIsStored` must fail.
module remoteAckEarly {
  import remoteLink(ACK_EARLY = true).*
}
