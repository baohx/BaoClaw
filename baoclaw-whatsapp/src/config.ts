/**
 * Config loader for WhatsApp Gateway.
 * Reads the `whatsapp` section from ~/.baoclaw/config.json.
 */
import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';

export interface WhatsAppConfig {
  enabled: boolean;
  phoneNumber: string | null;
  allowFrom: string[];
  dmPolicy: 'allow' | 'ignore';
  groupPolicy: 'allow' | 'ignore';
}

export const DEFAULTS: WhatsAppConfig = {
  enabled: false,
  phoneNumber: null,
  allowFrom: [],
  dmPolicy: 'allow',
  groupPolicy: 'ignore',
};

export function defaultConfigPath(): string {
  return path.join(os.homedir(), '.baoclaw', 'config.json');
}

/**
 * Load the whatsapp section from the config file.
 * Missing file or invalid JSON returns defaults.
 * Missing fields are filled with defaults; unknown fields are preserved in the raw object.
 */
export function loadWhatsAppConfig(configPath?: string): WhatsAppConfig {
  const filePath = configPath ?? defaultConfigPath();
  try {
    const raw = JSON.parse(fs.readFileSync(filePath, 'utf-8'));
    const wa = raw?.whatsapp;
    if (!wa || typeof wa !== 'object') return { ...DEFAULTS };
    return {
      enabled: typeof wa.enabled === 'boolean' ? wa.enabled : DEFAULTS.enabled,
      phoneNumber: typeof wa.phoneNumber === 'string' ? wa.phoneNumber : DEFAULTS.phoneNumber,
      allowFrom: Array.isArray(wa.allowFrom) ? wa.allowFrom : DEFAULTS.allowFrom,
      dmPolicy: wa.dmPolicy === 'allow' || wa.dmPolicy === 'ignore' ? wa.dmPolicy : DEFAULTS.dmPolicy,
      groupPolicy: wa.groupPolicy === 'allow' || wa.groupPolicy === 'ignore' ? wa.groupPolicy : DEFAULTS.groupPolicy,
    };
  } catch {
    return { ...DEFAULTS };
  }
}

/**
 * Watch the config file for changes and invoke onChange with the new config.
 * Returns the FSWatcher so the caller can close it.
 */
export function watchConfig(
  configPath: string,
  onChange: (config: WhatsAppConfig) => void,
): fs.FSWatcher {
  let debounce: ReturnType<typeof setTimeout> | null = null;
  return fs.watch(configPath, () => {
    if (debounce) clearTimeout(debounce);
    debounce = setTimeout(() => {
      onChange(loadWhatsAppConfig(configPath));
    }, 500);
  });
}
