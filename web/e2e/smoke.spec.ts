import { expect, type Page, test } from '@playwright/test';
import { PASSWORD, SPEAKERS, USERNAME } from './speakers';

test.skip(!process.env.TEST_SERVER_URL, 'TEST_SERVER_URL is not set');

const [KITCHEN, BEDROOM] = SPEAKERS;

async function signIn(page: Page) {
  await page.goto('/');
  await page.getByLabel('Username').fill(USERNAME);
  await page.getByLabel('Password').fill(PASSWORD);
  await page.getByRole('button', { name: 'Sign in' }).click();
  // Home first, with what came into the library lately.
  await expect(page.getByText('Recently added')).toBeVisible();
  await page.goto('/players');
  await expect(page.getByText('Every speaker the server knows')).toBeVisible();
}

test.beforeEach(async ({ page }) => {
  await signIn(page);
});

test('lists the speakers', async ({ page }) => {
  await expect(page.getByRole('link', { name: KITCHEN.name, exact: true })).toBeVisible();
  await expect(page.getByRole('link', { name: BEDROOM.name, exact: true })).toBeVisible();
});

test('plays an album on a speaker, and its queue shows it', async ({ page }) => {
  await page.goto('/library');
  await page.getByRole('link', { name: 'First Album', exact: true }).click();
  await page
    // The page's and the now-playing bar's choose the same player.
    .getByRole('combobox', { name: 'Plays on' })
    .first()
    .selectOption({ label: KITCHEN.name });
  // The album's Play, not the now-playing bar's, which resumes what played before.
  await page.getByRole('main').getByRole('button', { name: 'Play', exact: true }).click();
  // The toast comes once the server has started the speaker, which can take a few seconds.
  await expect(page.getByRole('alert', { name: `Playing on ${KITCHEN.name}` })).toBeVisible({
    timeout: 15_000,
  });

  await page.goto('/queues');
  // The queue's row, not the now-playing bar's link to the same queue.
  await page.getByRole('link', { name: new RegExp(`^${KITCHEN.name}(?!'s queue)`) }).click();
  await expect(page.getByText(/^(Playing|Starting)$/)).toBeVisible();
  await expect(page.getByText('Opening', { exact: true }).first()).toBeVisible();
});

test('adds an album to a queue, and moves one of its tracks up', async ({ page, isMobile }) => {
  // The server moves nothing up to what the player already holds, so the album is added
  // after what plays, where its tracks can move.
  await page.goto('/library');
  await page.getByRole('link', { name: 'Second Album', exact: true }).click();
  await page
    // The page's and the now-playing bar's choose the same player.
    .getByRole('combobox', { name: 'Plays on' })
    .first()
    .selectOption({ label: KITCHEN.name });
  await page.getByRole('main').getByRole('button', { name: 'Add to queue', exact: true }).click();
  await expect(page.getByRole('alert', { name: `Added to ${KITCHEN.name}'s queue` })).toBeVisible({
    timeout: 15_000,
  });

  await page.goto(`/queues/${KITCHEN.id}`);
  const titles = page.getByText(/^(Alpha|Beta|Gamma)$/);
  await expect(titles.last()).toBeVisible();
  // A phone keeps an item's actions behind More actions.
  if (isMobile) await page.getByRole('button', { name: 'More actions for Gamma' }).click();
  await page.getByRole('button', { name: 'Move Gamma up' }).click();
  await expect
    .poll(async () => {
      const order = await titles.allTextContents();
      return order.indexOf('Gamma') < order.indexOf('Beta');
    })
    .toBe(true);
});

test('groups one speaker with another, and takes it out again', async ({ page }) => {
  await page.goto('/players');
  await page.getByRole('button', { name: `More controls for ${BEDROOM.name}` }).click();
  await page
    .getByRole('combobox', { name: `Group ${BEDROOM.name} with` })
    .selectOption({ label: KITCHEN.name });
  const leave = page.getByRole('button', { name: `Take ${BEDROOM.name} out of the group` });
  await expect(leave).toBeVisible();
  await leave.click();
  await expect(leave).toBeHidden();
  await expect(
    page.getByRole('button', { name: `More controls for ${BEDROOM.name}` }),
  ).toBeVisible();
});

test('opens an artist from their album', async ({ page }) => {
  await page.goto('/library');
  await page.getByRole('link', { name: 'First Album', exact: true }).click();
  await page.getByRole('link', { name: 'Test Artist' }).click();
  await expect(page.getByText('ARTIST', { exact: true })).toBeVisible();
  await expect(page.getByRole('link', { name: 'First Album', exact: true })).toBeVisible();
});

test('the now-playing bar shows what plays, and pauses it', async ({ page }) => {
  await page.goto('/library');
  await page.getByRole('link', { name: 'First Album', exact: true }).click();
  await page
    .getByRole('combobox', { name: 'Plays on' })
    .first()
    .selectOption({ label: KITCHEN.name });
  await page.getByRole('main').getByRole('button', { name: 'Play', exact: true }).click();
  const bar = page.getByRole('region', { name: 'Now playing' });
  await expect(bar.getByText('Opening', { exact: true })).toBeVisible();
  await bar.getByRole('button', { name: 'Pause' }).click();
  await expect(bar.getByRole('button', { name: 'Play' })).toBeVisible();
});

test('shows how speakers are kept in step, and moves one', async ({ page }) => {
  await page.goto('/sync');
  await expect(page.getByText('AirPlay beside other protocols')).toBeVisible();
  const field = page.getByLabel(`${KITCHEN.name} over`, { exact: false }).first();
  await expect(field).toBeVisible();
  await field.fill('7');
  await field.press('Enter');
  await expect(page.getByText(`${KITCHEN.name} waits 7 ms from the next play`)).toBeVisible();
  await page.getByRole('button', { name: `Clear ${KITCHEN.name}'s adjustment` }).click();
  await expect(field).toHaveValue('0');
});
