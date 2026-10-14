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
- Two LinkPlay speakers, such as a WiiM and an Audio Pro, for the LinkPlay checks. Note the
  input each is on and its volume first, to put back afterwards.
- The Spotify app, signed in to a Premium account, for the Spotify Connect checks.
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

### LinkPlay

18. **LinkPlay solo.** Play to a LinkPlay speaker. It is audible within about four seconds, and
    Bopsie's position follows what is heard. Log at debug: `audible relative to the timeline`
    with `off_ms` within a few hundred.
19. **LinkPlay group.** Add the second LinkPlay speaker in Bopsie's output sheet. It plays the
    same audio as the leader within a few seconds; Bopsie shows it following. Remove it: it
    stops, the leader plays on.
20. **A group made in the vendor's app.** Group the two in the vendor's app. Within ten seconds
    Bopsie shows the follower following; ungroup them in the app and it shows that too.
21. **AirPlay it withdrew.** With a LinkPlay speaker that has AirPlay following another, add it
    to an AirPlay group in Bopsie. It leaves the LinkPlay group and joins the AirPlay group
    within half a minute.
22. **Afterwards.** Put each LinkPlay speaker back on its input: playing to it switched it to
    the network.

### The library

23. **An unmounted folder.** Unmount (or rename) a music folder, then `music/sync`. Bopsie still
    lists its music. Mount it again and sync: nothing changed. Log: `scan failed` naming the
    provider, `it lists nothing`.

### Settings, from the web UI

24. **Sync adjust by ear.** Group two AirPlay speakers, stand between them, and move sync adjust
    on one in steps of 5 ms from its player page, playing again after each change. The echo
    narrows to none; the setting shows as saved.
25. **A maximum volume.** Set a speaker's maximum below its volume: it turns down at once. Turn
    it up from Bopsie, and on its own app or remote: within a few seconds it is back at the
    maximum. Log: `turned down to its maximum`.
26. **A protocol turned off.** For a speaker reached over two protocols, turn off the one it
    plays over while it plays: it plays on over the other. Turn the other off too: the player
    shows unavailable.
27. **PIN pairing.** On the PIN speaker's page, Pair shows a PIN on the speaker; enter it. The
    speaker becomes available and plays.

### A speaker's own controls

28. **AirPlay buttons.** Group an AirPlay speaker with a LinkPlay speaker and play. Press pause
    on the AirPlay speaker or its remote: both go quiet, and Bopsie shows the group paused.
    Press play: both play on. Next skips the group's queue. Log: `the speaker's own controls`
    with `Pause`, then `Play`.
29. **LinkPlay buttons.** Play to a LinkPlay speaker and press pause on it or in its app: Bopsie
    shows the group paused within a couple of seconds, and the speaker stays paused rather
    than stopped. Press play on it: the group plays on. Log: `the speaker's own controls`.
30. **Another input.** While a LinkPlay speaker plays in a group, switch it to another input
    from its remote or app. It leaves the group, which plays on, and stays on the input it
    went to. Log: `the speaker's own controls` with `Released`.
31. **A Sendspin controller.** On an Android phone playing from Bopsie, use the notification's
    controls: pause, play, next and the progress bar act on the group the phone plays in. Log:
    `the speaker's own controls` with the phone's id.

### Spotify Connect

32. **Picking a speaker.** In the Spotify app, pick an AirPlay speaker and play. It is audible
    within a few seconds; Bopsie shows the track with its artwork, and the queue screen says it
    plays from Spotify. Log: `picked in the Spotify app`, then `playing a source`.
33. **Pause, play and seek.** Pause in Spotify: the speaker goes quiet at once. Play: it picks up
    where it went quiet, not seconds later. Seek: it plays from there within a few seconds.
34. **A group.** Add a LinkPlay speaker to the group in Bopsie while Spotify plays: it joins in
    sync. Press pause on one speaker: Spotify shows paused.
35. **Volume.** Move Spotify's volume slider: the group follows, within each speaker's maximum.
    Move the group's volume in Bopsie: Spotify's slider follows.
36. **Taking the group back.** Play an album from Bopsie on the group: Spotify lets go and the
    album plays. Log: `the queue plays again`.
37. **Turned off.** Turn Spotify Connect off on a speaker's page: within a few seconds it is gone
    from the Spotify app's devices. Turn it back on.
38. **After a restart.** Restart the server, then pick the device in Spotify again: it plays
    without being picked twice or signed in again.
