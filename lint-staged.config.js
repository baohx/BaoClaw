export default {
  // 1. Web / TS Tarafı: Biçimlendirme, Lint ve Tip Kontrolü
  "*.{js,jsx}": ["prettier --write", "eslint --fix"],
  "*.{json,md,yml,yaml}": ["prettier --write"],
  // TS dosyalarında değişiklik varsa tüm projenin tip doğrulaması çalışır
  "*.{ts,tsx}": (filenames) => {
    const formatCmds = filenames.map((f) => `prettier --write ${f}`);
    const lintCmds = filenames.map((f) => `eslint --fix ${f}`);

    const packages = [
      "baoclaw-web",
      "baoclaw-feishu",
      "baoclaw-whatsapp",
      "baoclaw-telegram",
      "ts-ipc",
    ];
    const typecheckCmds = [];
    const affectedPkgs = new Set();

    for (const f of filenames) {
      for (const pkg of packages) {
        if (f.startsWith(pkg + "/")) {
          affectedPkgs.add(pkg);
        }
      }
    }

    // Her etkilenen paket için kendi typecheck'i
    for (const pkg of affectedPkgs) {
      typecheckCmds.push(`cd ${pkg} && npm run typecheck`);
    }

    // Kök seviyesindeki .ts/.tsx dosyaları için kök typecheck
    const hasRootTs = filenames.some((f) => {
      const parts = f.split("/");
      return parts.length <= 1 || !packages.includes(parts[0]);
    });
    if (hasRootTs || affectedPkgs.size === 0) {
      typecheckCmds.push("npm run typecheck");
    }

    return [...formatCmds, ...lintCmds, ...typecheckCmds];
  },

  // 2. Rust Tarafı: Otomatik Format, Clippy ve Cargo Check
  "*.rs": (filenames) => {
    const formatCmds = filenames.map((f) => `rustfmt --edition 2021 ${f}`);
    const checkCmds = [
      ...filenames.map((f) => `rustfmt --edition 2021 --check ${f}`),
      "cargo clippy --manifest-path baoclaw-core/Cargo.toml --all-targets --all-features",
      "cargo check --manifest-path baoclaw-core/Cargo.toml --all-targets --all-features",
    ];
    return [...formatCmds, ...checkCmds];
  },
};
