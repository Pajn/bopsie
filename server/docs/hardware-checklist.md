# Hardware checklist

Run by hand before a release, against real speakers. The automated suites cover the protocols
against stand-ins; this covers what only real speakers show: how they sound, how they keep
time, and how they recover.

## What you need

- The server running on Linux, as the release will run it (the image or the systemd service),
  on the same network segment as the speakers. PTP does not cross subnets.
- An AirPlay 2 speaker, and a second one for the group checks.
- An AirPlay speaker that pairs with a PIN, such as an Apple TV or a HomePod with restricted
  access, for the pairing check. Skip that check without one.
- A phone with Bopsie, signed in to the server.
- The server's log at `info`, with `RUST_LOG=info,bopsie_players_airplay=debug` for the AirPlay
  checks, to read the lines quoted below.

Keep volumes low: each check needs the speaker audible, not loud.

## Checks

Each line says what to do, what to hear, and what the log shows when it went right.

### Playing

1. **Solo play.** Play an album to one AirPlay speaker from Bopsie. It is audible within a few
   seconds; tracks follow each other without a gap. Log: `cliairplay started`, then
   `[STATUS] started` with `requested_unix_ms` equal to `at_unix_ms`.
2. **Seek.** Seek within a track. Playback continues from the new place within about a second;
   the log shows `[STATUS] flushed`, then a new `[STATUS] started`.
3. **Volume.** Change the volume in Bopsie. The speaker follows without a break in the audio,
   with no `[STATUS] flushed`.
4. **Phone.** Play to the phone itself. It plays; pause and resume pick up where it was.

### Pairing

5. **PIN pairing.** Send `airplay/pair_start` for the PIN speaker; it shows a PIN. Send
   `airplay/pair_finish` with it. The speaker becomes available (log: `paired`) and plays.
   Restart the server: it still plays without pairing again.

### Groups

6. **Group of two.** Play to one AirPlay speaker, then add the second in Bopsie's output sheet.
   The second joins within a few seconds, in step with the first: no echo standing near both.
   Log for the second: `[STATUS] started` on time.
7. **Group volume.** Move the group volume. Both follow, keeping their balance, without a break.
8. **Leave.** Remove the second speaker. It stops; the first plays on.
9. **Mixed protocols.** Try adding the phone to the AirPlay group. Bopsie does not offer it;
   through the API, `players/cmd/group` fails with the reason (error 9).
10. **Transfer.** Move the queue from one speaker to the other. It continues on the new one from
    where it was, and the old one stops.

### Recovery

11. **A speaker's power.** While a group plays, unplug one speaker; plug it back in. The rest
    play on throughout. Once the speaker is back on the network it rejoins in step on its own.
    Log: `an output failed; it rejoins after a wait`, then a new `cliairplay started` for it.
12. **A crashed process.** While playing, kill a speaker's `cliairplay` process (`kill -9`).
    It comes back within a few seconds and rejoins in step. Log: `cliairplay exited`, then
    `an output failed; it rejoins after a wait`.
13. **The PTP daemon.** Kill the `cliairplay --ptp-daemon` process. Log:
    `the PTP daemon stopped; starting it again`, then `PTP daemon running`. A group started
    after that is in step.
14. **The phone's network.** While the phone plays its own queue, switch it between Wi-Fi and
    mobile data. It plays on once it reconnects. Log: `the group's leader is gone; pausing`,
    then `the group's leader is back; playing on`.
15. **Restart.** Mid-queue, with a group formed and volumes set, restart the server
    (`systemctl restart bopsie`, or `docker restart`). The speakers stop cleanly. After the
    restart Bopsie shows the same queue, idle where it was; the group forms again once both
    speakers are found; volumes are as they were. Play resumes from where it stopped.
16. **A crash.** Kill the server with `kill -9` while it plays, then start it. Log:
    `ended cliairplay processes an earlier run left`, then `PTP daemon running`. Playing works.

### The library

17. **An unmounted folder.** Unmount (or rename) a music folder, then `music/sync`. Bopsie still
    lists its music. Mount it again and sync: nothing changed. Log: `scan failed` naming the
    provider, `it lists nothing`.
