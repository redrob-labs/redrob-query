// Verify a Tauri updater signature against the bytes it claims to cover.
//
// Why this exists: the release's "Updater contract" job only asserted that every latest.json entry
// HAS a signature. A signature over the wrong bytes passes that and fails on every user's machine at
// the first update check. Measured with the updater's own crate (minisign-verify 0.2.5) before
// writing this: a signature made for a repacked AppImage is VALID for the repacked bytes and
// INVALID for the original ones -- so "non-empty" proves nothing.
//
// No dependency: Node's crypto has Ed25519 and BLAKE2b-512, which is all minisign needs. The checks
// are the ones minisign-verify performs, in the same order:
//   1. key id in the signature equals the public key's id
//   2. Ed25519 over BLAKE2b-512(file)  ("ED", pre-hashed)  or over the raw file ("Ed", legacy)
//   3. the global signature over (signature || trusted comment), so the comment cannot be swapped
//
// Usage: node scripts/verify-updater-signature.mjs <public-key> <signature> <file>
//   <public-key> and <signature> are either file paths or the base64 strings themselves, which is
//   how latest.json and the TAURI_SIGNING_PUBLIC_KEY variable carry them.
import { createHash, createPublicKey, verify } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';

const [pubArg, sigArg, filePath] = process.argv.slice(2);
if (!pubArg || !sigArg || !filePath) {
  console.error('usage: verify-updater-signature.mjs <public-key> <signature> <file>');
  process.exit(2);
}

// Tauri wraps the minisign text in one more layer of base64.
const unwrap = (arg) => {
  const raw = existsSync(arg) ? readFileSync(arg, 'utf8') : arg;
  return Buffer.from(raw.trim(), 'base64').toString('utf8');
};
// A minisign file is "untrusted comment: ..." then a base64 line, and for signatures a trusted
// comment line and a second base64 line.
const lines = (text) => text.split('\n').map((line) => line.trim()).filter(Boolean);

const fail = (why) => {
  console.log(`INVALID: ${why}`);
  process.exit(1);
};

const pubLines = lines(unwrap(pubArg));
const pubBytes = Buffer.from(pubLines[1] ?? '', 'base64');
if (pubBytes.length !== 42 || pubBytes.subarray(0, 2).toString('latin1') !== 'Ed') {
  fail('public key is not a minisign Ed25519 key');
}
const pubKeyId = pubBytes.subarray(2, 10);
// A raw 32-byte Ed25519 key needs the fixed SPKI prefix to become a KeyObject.
const publicKey = createPublicKey({
  key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), pubBytes.subarray(10)]),
  format: 'der',
  type: 'spki',
});

const sigLines = lines(unwrap(sigArg));
const sigBytes = Buffer.from(sigLines[1] ?? '', 'base64');
const trusted = sigLines[2] ?? '';
const globalSig = Buffer.from(sigLines[3] ?? '', 'base64');
if (sigBytes.length !== 74) fail('signature line is not 74 bytes');
if (!trusted.startsWith('trusted comment: ')) fail('signature has no trusted comment');
if (globalSig.length !== 64) fail('global signature is not 64 bytes');

const algorithm = sigBytes.subarray(0, 2).toString('latin1');
const sigKeyId = sigBytes.subarray(2, 10);
const signature = sigBytes.subarray(10);

if (!sigKeyId.equals(pubKeyId)) {
  fail(`signed by key ${sigKeyId.toString('hex')}, not ${pubKeyId.toString('hex')}`);
}

const data = readFileSync(filePath);
let message;
if (algorithm === 'ED') message = createHash('blake2b512').update(data).digest();
else if (algorithm === 'Ed') message = data;
else fail(`unknown signature algorithm ${JSON.stringify(algorithm)}`);

if (!verify(null, message, publicKey, signature)) fail('file signature does not match these bytes');

const commentBody = Buffer.from(trusted.slice('trusted comment: '.length), 'utf8');
if (!verify(null, Buffer.concat([signature, commentBody]), publicKey, globalSig)) {
  fail('global signature over the trusted comment does not verify');
}

console.log('VALID');
