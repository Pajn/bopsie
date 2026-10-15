import { byId, byText, expectText, expectVisible, fill, tap } from './actions';
import { freshApp, launchApp } from './app';
import { APP_SERVER_URL, PASSWORD, USERNAME } from './server';

describe('sign in', () => {
  beforeAll(async () => {
    await freshApp();
    await expectVisible(byId('login-server'), { timeout: 20_000 });
  });

  it('rejects a wrong password and then signs in with the right one', async () => {
    await fill(byId('login-server'), APP_SERVER_URL);
    await fill(byId('login-username'), USERNAME);
    await fill(byId('login-password'), 'definitely-wrong');
    await tap(byId('login-submit'));
    await expectVisible(byId('login-error'), { timeout: 20_000 });

    await fill(byId('login-password'), PASSWORD);
    await tap(byId('login-submit'));
    await expectVisible(byId('home'), { timeout: 20_000 });
    await tap(byId('tab-account'));
    await expectText(byId('account-username'), USERNAME);
  });

  it('stays signed in across a relaunch and can sign out', async () => {
    await launchApp();
    await expectVisible(byId('home'), { timeout: 20_000 });
    await tap(byId('tab-account'));
    await tap(byId('account-sign-out'));
    // The confirmation covers the screen, so its button is the one in view.
    await tap(byText('Sign out'));
    await expectVisible(byId('login-server'), { timeout: 20_000 });
  });
});
