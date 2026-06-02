# AES-CBC Shim Source Files

These are the source-of-truth copies of `_aes_cbc_shim.js` (ESM) and
`_aes_cbc_shim.cjs` (CJS), maintained alongside the patch-package patches that
inject them into `node_modules`.

## Why this exists

On Zhaoxin KaiXian KX-7000 (and other VIA-derived x86 CPUs), the OpenSSL 3.x
AES-NI / Padlock code path produces wrong output when DECRYPTING AES-256-CBC,
AES-192-CBC, and AES-256-ECB. Encrypt is correct; the decrypt-side key
schedule is broken. CTR and GCM modes (which use only AES encrypt internally)
are unaffected.

See https://github.com/openssl/openssl/issues/20073

System OpenSSL 1.1.1d works fine on the same CPU, but Node.js bundles its own
OpenSSL 3.x and `OPENSSL_ia32cap` cannot disable the broken path.

## What gets patched

The `patches/` directory contains diffs for `@whiskeysockets/baileys` and
`libsignal`. They:

1. Add `_aes_cbc_shim.js` to each library directory.
2. Replace `crypto.createCipheriv`/`createDecipheriv` calls for the affected
   algorithms with calls into the shim. The shim auto-detects the broken
   hardware at runtime; on healthy CPUs it transparently delegates to native
   `crypto`, so the patched code remains correct on any host.

## How to update

If you change the shim source here, also update the relevant patch files by
running:

    npx patch-package @whiskeysockets/baileys
    npx patch-package libsignal

after copying the new shim into `node_modules/.../{Utils,src}/_aes_cbc_shim.js`.
