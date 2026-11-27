// Release assets are pinned by version/asset ID and SHA-256, including the AppImage runtime.
import { createHash } from "node:crypto";
import { chmodSync, existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const tools = [
  [
    "linuxdeploy.AppImage",
    ["https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage"],
    "c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d",
  ],
  [
    "appimagetool.AppImage",
    ["https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage"],
    "ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0",
  ],
  [
    "runtime-x86_64",
    [
      // Prefer the direct release download URL so GitHub-hosted runners do not hit
      // api.github.com's shared IP rate limit, while keeping the pinned asset ID as fallback.
      "https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-x86_64",
      "https://api.github.com/repos/AppImage/type2-runtime/releases/assets/657374641",
    ],
    "617aa77314d57c8b8e4fae9c3f0b35af7b30d5529cb69e98f64f92510ed98a46",
  ],
];

const MAX_ATTEMPTS = 4;
const MAX_REDIRECTS = 5;
const TIMEOUT_MS = 120_000;
const token = process.env.GH_TOKEN || process.env.GITHUB_TOKEN || "";

const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

function requestHeaders(url) {
  const headers = {
    Accept: "application/octet-stream",
    "User-Agent": "winc-release-tools",
  };
  if (token && new URL(url).hostname === "api.github.com") {
    headers.Authorization = `Bearer ${token}`;
    headers["X-GitHub-Api-Version"] = "2022-11-28";
  }
  return headers;
}

async function fetchWithRedirects(initialUrl) {
  let currentUrl = initialUrl;
  for (let hop = 0; hop <= MAX_REDIRECTS; hop += 1) {
    const response = await fetch(currentUrl, {
      headers: requestHeaders(currentUrl),
      redirect: "manual",
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
    if (response.status >= 300 && response.status < 400) {
      const location = response.headers.get("location");
      if (!location) {
        throw new Error(`redirect (${response.status}) without Location header from ${currentUrl}`);
      }
      await response.body?.cancel();
      currentUrl = new URL(location, currentUrl).toString();
      continue;
    }
    return response;
  }
  throw new Error(`too many redirects while downloading ${initialUrl}`);
}

function retryDelayMs(response, attempt) {
  const retryAfter = response?.headers.get("retry-after");
  if (retryAfter && /^\d+$/.test(retryAfter)) {
    return Math.min(Math.max(Number(retryAfter) * 1000, 1000), 60_000);
  }
  return 2000 * 2 ** (attempt - 1);
}

function isRetryableStatus(status) {
  return status === 403 || status === 408 || status === 429 || status >= 500;
}

async function downloadUrl(name, url) {
  let lastError;
  for (let attempt = 1; attempt <= MAX_ATTEMPTS; attempt += 1) {
    let response;
    try {
      response = await fetchWithRedirects(url);
    } catch (error) {
      lastError = error instanceof Error ? error : new Error(String(error));
      if (attempt === MAX_ATTEMPTS) throw lastError;
      const waitMs = retryDelayMs(null, attempt);
      console.error(`${name}: attempt ${attempt}/${MAX_ATTEMPTS} failed (${lastError.message}); retrying in ${waitMs}ms`);
      await sleep(waitMs);
      continue;
    }

    if (response.ok) {
      try {
        return Buffer.from(await response.arrayBuffer());
      } catch (error) {
        lastError = error instanceof Error ? error : new Error(String(error));
        if (attempt === MAX_ATTEMPTS) throw lastError;
        const waitMs = retryDelayMs(null, attempt);
        console.error(`${name}: attempt ${attempt}/${MAX_ATTEMPTS} body read failed (${lastError.message}); retrying in ${waitMs}ms`);
        await sleep(waitMs);
        continue;
      }
    }

    const remaining = response.headers.get("x-ratelimit-remaining");
    const rateInfo = remaining !== null ? `, rate-limit-remaining=${remaining}` : "";
    const detail = `${name}: download failed (${response.status}${rateInfo}) from ${url}`;
    await response.body?.cancel();
    lastError = new Error(detail);
    if (!isRetryableStatus(response.status) || attempt === MAX_ATTEMPTS) {
      throw lastError;
    }
    const waitMs = retryDelayMs(response, attempt);
    console.error(`${detail}; retrying in ${waitMs}ms (attempt ${attempt}/${MAX_ATTEMPTS})`);
    await sleep(waitMs);
  }
  throw lastError;
}

async function downloadVerified(name, urls, digest) {
  const errors = [];
  for (const url of urls) {
    try {
      const bytes = await downloadUrl(name, url);
      const actual = hash(bytes);
      if (actual === digest) return bytes;
      const message = `${name}: SHA-256 mismatch from ${url} (expected ${digest}, got ${actual})`;
      console.error(message);
      errors.push(message);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      console.error(message);
      errors.push(message);
    }
  }
  throw new Error(errors.join("; "));
}

const directory = resolve("target/release-tools");
mkdirSync(directory, { recursive: true });
for (const [name, urls, digest] of tools) {
  const file = resolve(directory, name);
  if (!existsSync(file) || hash(readFileSync(file)) !== digest) {
    const bytes = await downloadVerified(name, urls, digest);
    writeFileSync(`${file}.part`, bytes);
    renameSync(`${file}.part`, file);
  }
  chmodSync(file, 0o755);
}
