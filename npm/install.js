#!/usr/bin/env node
// postinstall: downloads the signaldock binary from GitHub Releases

'use strict';

const https = require('https');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { spawnSync } = require('child_process');

const REPO = 'CleoAgent/signaldock-runtime';
const VERSION = require('./package.json').version;
const BIN_DIR = path.join(__dirname, 'bin');
const NATIVE_BIN = path.join(BIN_DIR, process.platform === 'win32' ? 'signaldock-native.exe' : 'signaldock-native');

function getPlatformId() {
  const plat = process.platform;
  const arch = process.arch;

  if (plat === 'linux' && arch === 'x64') return 'linux-x64';
  if (plat === 'darwin' && arch === 'x64') return 'darwin-x64';
  if (plat === 'darwin' && arch === 'arm64') return 'darwin-arm64';
  if (plat === 'win32' && arch === 'x64') return 'windows-x64';

  throw new Error(
    `Unsupported platform: ${plat}/${arch}.\n` +
      'Supported: linux/x64, darwin/x64, darwin/arm64, win32/x64'
  );
}

function getDownloadUrl(platformId) {
  const suffix = platformId.startsWith('windows') ? '.zip' : '.tar.gz';
  return `https://github.com/${REPO}/releases/download/v${VERSION}/signaldock-${platformId}${suffix}`;
}

function downloadFile(url, dest) {
  return new Promise((resolve, reject) => {
    function get(nextUrl) {
      https
        .get(nextUrl, (res) => {
          if (res.statusCode === 301 || res.statusCode === 302) {
            return get(res.headers.location);
          }
          if (res.statusCode !== 200) {
            return reject(new Error(`HTTP ${res.statusCode} downloading ${nextUrl}`));
          }

          const tmp = `${dest}.tmp`;
          const out = fs.createWriteStream(tmp);
          res.pipe(out);
          out.on('finish', () => {
            out.close(() => {
              fs.renameSync(tmp, dest);
              resolve();
            });
          });
          out.on('error', (err) => {
            fs.unlink(tmp, () => {});
            reject(err);
          });
        })
        .on('error', reject);
    }
    get(url);
  });
}

function extractArchive(archivePath, tmpDir) {
  if (process.platform === 'win32') {
    const result = spawnSync(
      'powershell',
      ['-NoProfile', '-Command', `Expand-Archive -LiteralPath '${archivePath}' -DestinationPath '${tmpDir}' -Force`],
      { stdio: 'inherit' }
    );
    if (result.status !== 0) {
      throw new Error('failed to extract zip archive with powershell');
    }
    return;
  }

  const result = spawnSync('tar', ['xzf', archivePath, '-C', tmpDir], { stdio: 'inherit' });
  if (result.status !== 0) {
    throw new Error('failed to extract tar.gz archive with tar');
  }
}

async function main() {
  if (process.env.SIGNALDOCK_SKIP_DOWNLOAD === '1') {
    console.log('signaldock: skipping binary download (SIGNALDOCK_SKIP_DOWNLOAD=1)');
    return;
  }

  if (fs.existsSync(NATIVE_BIN)) {
    console.log('signaldock: binary already present, skipping download.');
    return;
  }

  let platformId;
  try {
    platformId = getPlatformId();
  } catch (err) {
    console.error(`signaldock install error: ${err.message}`);
    process.exit(1);
  }

  const url = getDownloadUrl(platformId);
  console.log(`signaldock: downloading v${VERSION} for ${platformId}...`);
  console.log(`  from: ${url}`);

  fs.mkdirSync(BIN_DIR, { recursive: true });
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'signaldock-'));
  const archivePath = path.join(tmpDir, path.basename(url));

  try {
    await downloadFile(url, archivePath);
    extractArchive(archivePath, tmpDir);
  } catch (err) {
    console.error(`signaldock: download failed: ${err.message}`);
    console.error('You can manually download the release from:');
    console.error(`  https://github.com/${REPO}/releases/tag/v${VERSION}`);
    process.exit(1);
  }

  const extractedName = process.platform === 'win32' ? 'signaldock.exe' : 'signaldock';
  const extractedPath = path.join(tmpDir, extractedName);
  if (!fs.existsSync(extractedPath)) {
    console.error(`signaldock: extracted binary not found at ${extractedPath}`);
    process.exit(1);
  }

  fs.copyFileSync(extractedPath, NATIVE_BIN);
  if (process.platform !== 'win32') {
    fs.chmodSync(NATIVE_BIN, 0o755);
  }

  console.log(`signaldock: installed to ${NATIVE_BIN}`);
}

main();
