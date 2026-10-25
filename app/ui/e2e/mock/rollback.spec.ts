// A volume that opens older than this computer last saw it (VaultInfo.rolled_back).
import { boot, expect, test } from "./helpers";

test("rollback notice: shown for an older copy, gone once the version is kept", async ({
  page,
}) => {
  await boot(page, "screen=vault&rolledback=1");
  const n = page.getByTestId("rollback-notice");
  await expect(n).toContainText("copia più vecchia");
  await n.getByRole("button", { name: "Tieni questa versione" }).click();
  await expect(n).toHaveCount(0);
});

test('rollback notice: not shown for an up-to-date volume, "Non ora" hides it', async ({
  page,
}) => {
  await boot(page, "screen=vault");
  await expect(page.getByTestId("rollback-notice")).toHaveCount(0);
  await boot(page, "screen=vault&rolledback=1");
  await page
    .getByTestId("rollback-notice")
    .getByRole("button", { name: "Non ora" })
    .click();
  await expect(page.getByTestId("rollback-notice")).toHaveCount(0);
});
