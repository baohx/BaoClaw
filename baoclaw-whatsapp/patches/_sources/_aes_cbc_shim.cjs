/**
 * CJS variant of _aes_cbc_shim.js. Identical behaviour, exports via module.exports.
 * Used by libsignal which is CJS.
 *
 * See _aes_cbc_shim.js for full documentation.
 */
'use strict';

const nativeCrypto = require('crypto');
const aesjsRaw = require('aes-js');
const aesjs = aesjsRaw.default ?? aesjsRaw;

const PATCHED = new Set(['aes-256-cbc', 'aes-192-cbc', 'aes-256-ecb']);

function detectHardwareWorks() {
  try {
    const k = Buffer.alloc(32, 1);
    const iv = Buffer.alloc(16, 2);
    const c = nativeCrypto.createCipheriv('aes-256-cbc', k, iv);
    const enc = Buffer.concat([c.update(Buffer.from('test')), c.final()]);
    const d = nativeCrypto.createDecipheriv('aes-256-cbc', k, iv);
    const dec = Buffer.concat([d.update(enc), d.final()]);
    return dec.toString() === 'test';
  } catch {
    return false;
  }
}

const HARDWARE_BROKEN = !detectHardwareWorks();

if (HARDWARE_BROKEN && !globalThis.__AES_SHIM_LOGGED) {
  globalThis.__AES_SHIM_LOGGED = true;
  console.warn(
    '[aes-cbc-shim] Detected broken hardware AES-256/192 decrypt. ' +
    'Routing { aes-256-cbc, aes-192-cbc, aes-256-ecb } through pure-JS implementation.'
  );
}

function pkcs7Pad(data, blockSize) {
  const padLen = blockSize - (data.length % blockSize);
  return Buffer.concat([data, Buffer.alloc(padLen, padLen)]);
}

function pkcs7Unpad(data) {
  if (data.length === 0) return data;
  const padLen = data[data.length - 1];
  if (padLen < 1 || padLen > 16 || padLen > data.length) {
    const err = new Error('bad decrypt');
    err.code = 'ERR_OSSL_EVP_BAD_DECRYPT';
    throw err;
  }
  for (let i = data.length - padLen; i < data.length; i++) {
    if (data[i] !== padLen) {
      const err = new Error('bad decrypt');
      err.code = 'ERR_OSSL_EVP_BAD_DECRYPT';
      throw err;
    }
  }
  return data.subarray(0, data.length - padLen);
}

function toUint8Array(value) {
  if (typeof value === 'string') return new Uint8Array(Buffer.from(value, 'hex'));
  if (Buffer.isBuffer(value)) return new Uint8Array(value);
  return value;
}

class CbcShim {
  constructor(key, iv, encrypting) {
    this.key = key;
    this.iv = iv;
    this.encrypting = encrypting;
    this.chunks = [];
    this.autoPadding = true;
    this.finalized = false;
  }
  setAutoPadding(value = true) { this.autoPadding = value; return this; }
  update(data, inputEncoding) {
    if (this.finalized) throw new Error('Cipher already finalized');
    let buf;
    if (typeof data === 'string') buf = Buffer.from(data, inputEncoding ?? 'utf8');
    else if (Buffer.isBuffer(data)) buf = data;
    else buf = Buffer.from(data);
    this.chunks.push(buf);
    return Buffer.alloc(0);
  }
  final() {
    if (this.finalized) throw new Error('Cipher already finalized');
    this.finalized = true;
    const input = Buffer.concat(this.chunks);
    const cbc = new aesjs.ModeOfOperation.cbc(this.key, this.iv);
    if (this.encrypting) {
      const padded = this.autoPadding ? pkcs7Pad(input, 16) : input;
      if (padded.length % 16 !== 0) {
        const err = new Error('data not multiple of block length');
        err.code = 'ERR_OSSL_EVP_DATA_NOT_MULTIPLE_OF_BLOCK_LENGTH';
        throw err;
      }
      return Buffer.from(cbc.encrypt(padded));
    }
    if (input.length % 16 !== 0) {
      const err = new Error('wrong final block length');
      err.code = 'ERR_OSSL_EVP_WRONG_FINAL_BLOCK_LENGTH';
      throw err;
    }
    const decrypted = Buffer.from(cbc.decrypt(input));
    return this.autoPadding ? pkcs7Unpad(decrypted) : decrypted;
  }
}

class EcbShim {
  constructor(key, encrypting) {
    this.key = key;
    this.encrypting = encrypting;
    this.chunks = [];
    this.autoPadding = true;
    this.finalized = false;
  }
  setAutoPadding(value = true) { this.autoPadding = value; return this; }
  update(data, inputEncoding) {
    if (this.finalized) throw new Error('Cipher already finalized');
    let buf;
    if (typeof data === 'string') buf = Buffer.from(data, inputEncoding ?? 'utf8');
    else if (Buffer.isBuffer(data)) buf = data;
    else buf = Buffer.from(data);
    this.chunks.push(buf);
    return Buffer.alloc(0);
  }
  final() {
    if (this.finalized) throw new Error('Cipher already finalized');
    this.finalized = true;
    const input = Buffer.concat(this.chunks);
    const ecb = new aesjs.ModeOfOperation.ecb(this.key);
    if (this.encrypting) {
      const padded = this.autoPadding ? pkcs7Pad(input, 16) : input;
      if (padded.length % 16 !== 0) {
        const err = new Error('data not multiple of block length');
        err.code = 'ERR_OSSL_EVP_DATA_NOT_MULTIPLE_OF_BLOCK_LENGTH';
        throw err;
      }
      return Buffer.from(ecb.encrypt(padded));
    }
    if (input.length % 16 !== 0) {
      const err = new Error('wrong final block length');
      err.code = 'ERR_OSSL_EVP_WRONG_FINAL_BLOCK_LENGTH';
      throw err;
    }
    const decrypted = Buffer.from(ecb.decrypt(input));
    return this.autoPadding ? pkcs7Unpad(decrypted) : decrypted;
  }
}

function makeShim(algorithm, key, iv, encrypting) {
  const algo = algorithm.toLowerCase();
  const keyBytes = toUint8Array(key);
  if (algo === 'aes-256-ecb') return new EcbShim(keyBytes, encrypting);
  const ivBytes = iv == null ? new Uint8Array(16) : toUint8Array(iv);
  return new CbcShim(keyBytes, ivBytes, encrypting);
}

function createCipheriv(algorithm, key, iv, options) {
  if (HARDWARE_BROKEN && PATCHED.has(algorithm.toLowerCase())) {
    return makeShim(algorithm, key, iv, true);
  }
  return nativeCrypto.createCipheriv(algorithm, key, iv, options);
}

function createDecipheriv(algorithm, key, iv, options) {
  if (HARDWARE_BROKEN && PATCHED.has(algorithm.toLowerCase())) {
    return makeShim(algorithm, key, iv, false);
  }
  return nativeCrypto.createDecipheriv(algorithm, key, iv, options);
}

module.exports = {
  createCipheriv,
  createDecipheriv,
  createHash: nativeCrypto.createHash.bind(nativeCrypto),
  createHmac: nativeCrypto.createHmac.bind(nativeCrypto),
  randomBytes: nativeCrypto.randomBytes.bind(nativeCrypto),
  isPatchActive: () => HARDWARE_BROKEN,
};
