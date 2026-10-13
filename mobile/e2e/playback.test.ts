import { byId, expectText, expectVisible, fill, tap } from './actions';
import { freshApp } from './app';
import { APP_SERVER_URL, api, eventually, PASSWORD, USERNAME } from './server';

interface Player {
  player_id: string;
  provider: string;
  name: string;
  playback_state: string;
}

const sendspinPlayers = async () =>
  (await api<Player[]>('players/all')).filter((player) => player.provider === 'sendspin');

/**
 * Players registered before this run. Wiping the app gives it a new player
 * id, and the server keeps the old one, since an app that is wiped never
 * says goodbye.
 */
let earlier = new Set<string>();

/** The phone as the server sees it: the Sendspin player this install registered. */
async function phonePlayer(): Promise<Player | undefined> {
  return (await sendspinPlayers()).find((player) => !earlier.has(player.player_id));
}

describe('playback on the phone', () => {
  beforeAll(async () => {
    earlier = new Set((await sendspinPlayers()).map((player) => player.player_id));
    await freshApp();
    await fill(byId('login-server'), APP_SERVER_URL);
    await fill(byId('login-username'), USERNAME);
    await fill(byId('login-password'), PASSWORD);
    await tap(byId('login-submit'));
    await expectVisible(byId('library-list'), { timeout: 20_000 });
  });

  it('registers the phone as a Music Assistant player', async () => {
    await tap(byId('tab-account'));
    await expectText(
      byId('player-status'),
      'Ready. Music Assistant lists this phone as a player.',
      {
        timeout: 20_000,
      },
    );
    await eventually(async () => (await phonePlayer()) !== undefined);
    await tap(byId('tab-library'));
  });

  it('plays an album and shows what is playing', async () => {
    await tap(byId('album-1'));
    await expectText(byId('item-title'), 'First Album');
    await tap(byId('item-play'));
    await expectText(byId('mini-player-title'), 'Opening', { timeout: 20_000 });
    // The server only plays to a player that has synchronized its clock and
    // is taking audio, so this is the stream reaching the phone.
    await eventually(async () => (await phonePlayer())?.playback_state === 'playing');
  });

  it('pauses and skips from the now-playing screen', async () => {
    await tap(byId('mini-player'));
    await expectText(byId('player-title'), 'Opening');
    await tap(byId('player-next'));
    await expectText(byId('player-title'), 'Middle', { timeout: 20_000 });
    await tap(byId('player-toggle'));
    await eventually(async () => (await phonePlayer())?.playback_state !== 'playing');
  });
});
