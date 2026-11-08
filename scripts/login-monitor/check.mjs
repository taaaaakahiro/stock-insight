import { setTimeout as delay } from "node:timers/promises";
import { chromium } from "playwright";

const site = "https://stock.insight.maa-un.com";
const clerk = "https://clerk.maa-un.com";
const timeout = 12_000;
const requestOptions = { maxRedirects: 0, timeout };

// Only emit controlled stage names, reasons and status codes. Browser errors can contain OAuth URLs.
class CheckFailure extends Error {}

function requireThat(condition, reason) {
  if (!condition) throw new CheckFailure(reason);
}

async function checkGoogleHandoff(context) {
  const page = await context.newPage();
  await page.goto(site, { waitUntil: "domcontentloaded", timeout });
  requireThat(new URL(page.url()).origin === site, "unexpected sign-in page origin");
  const button = page.getByRole("button", { name: "Google でログイン", exact: true });
  await button.waitFor({ state: "visible", timeout }).catch(() => {
    throw new CheckFailure("Google sign-in button did not appear");
  });
  let clerkStatus;
  page.on("response", (response) => {
    const url = new URL(response.url());
    if (url.origin === clerk && url.pathname === "/v1/client/sign_ins") {
      clerkStatus = response.status();
    }
  });
  await button.click({ timeout });
  try {
    await page.waitForURL((url) => url.origin === "https://accounts.google.com", {
      waitUntil: "domcontentloaded",
      timeout,
    });
    await page.locator('input[autocomplete="username"]').waitFor({ state: "visible", timeout });
  } catch {
    const result = clerkStatus ? `Clerk sign-in HTTP ${clerkStatus}` : "no Clerk sign-in response";
    throw new CheckFailure(`Google account prompt did not appear (${result})`);
  }
}

async function checkCallback({ request }) {
  const response = await request.get(`${site}/sso-callback`, requestOptions);
  requireThat(response.ok(), `HTTP ${response.status()}`);
  requireThat(
    response.headers()["content-type"]?.startsWith("text/html") &&
      (await response.text()).includes('<div id="root">'),
    "callback URL did not serve the app shell",
  );
}

async function checkWorker({ request }) {
  const response = await request.get(`${site}/api/me`, requestOptions);
  requireThat(response.status() === 401, `expected HTTP 401, got ${response.status()}`);
  requireThat((await response.json()).error?.code === "unauthenticated", "unexpected API response");
}

async function checkSigningKeys({ request }) {
  const response = await request.get(`${clerk}/.well-known/jwks.json`, requestOptions);
  requireThat(response.ok(), `HTTP ${response.status()}`);
  const { keys } = await response.json();
  requireThat(
    Array.isArray(keys) && keys.some((key) => key?.kty === "RSA" && key.kid && key.n && key.e),
    "no RSA signing key returned",
  );
}

const checks = [
  ["Google sign-in", checkGoogleHandoff],
  ["SSO callback URL", checkCallback],
  ["Unauthenticated /api/me", checkWorker],
  ["Clerk signing keys", checkSigningKeys],
];

async function run(browser, pending) {
  const context = await browser.newContext({ locale: "ja-JP" });
  try {
    const results = await Promise.all(
      pending.map(async ([name, check]) => {
        try {
          await check(context);
          console.log(`OK: ${name}`);
          return false;
        } catch (error) {
          const reason = error instanceof CheckFailure ? error.message : "request or browser check failed";
          console.error(`FAIL: ${name}: ${reason}`);
          return true;
        }
      }),
    );
    return pending.filter((_, index) => results[index]);
  } finally {
    await context.close();
  }
}

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_EXECUTABLE_PATH });
try {
  const failed = await run(browser, checks);
  if (failed.length) {
    console.log("Retrying failed checks in a fresh browser context.");
    await delay(5_000);
    const remaining = await run(browser, failed);
    if (remaining.length) {
      console.error(`::error::Production login checks failed: ${remaining.map(([name]) => name).join(", ")}`);
      process.exitCode = 1;
    }
  }
} finally {
  await browser.close();
}
