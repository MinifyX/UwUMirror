/**
 * adb and the phones it knows, for the start page (paired phones to mirror)
 * and Settings → Android (pairing, adb itself).
 */

import { useCallback, useEffect, useState } from 'react';
import { api, errorText, type AdbStatus, type Device } from './api';
import { useSettings } from './settings';

/** How often the phone list is read while something shows it. */
const DEVICE_POLL_MS = 3000;

/** Which adb UwUMirror uses, chosen by setting or found; null while it looks. */
export function useAdb(): { adb: AdbStatus | null; refresh: () => void } {
  const { adbPath } = useSettings();
  const [adb, setAdb] = useState<AdbStatus | null>(null);
  const refresh = useCallback(() => {
    void api
      .androidChooseAdb(adbPath || null)
      .then(() => api.androidStatus())
      .then(setAdb)
      .catch(() => setAdb({ path: null, version: null, canDownload: false, own: false }));
  }, [adbPath]);
  useEffect(refresh, [refresh]);
  return { adb, refresh };
}

/** The phones adb knows, read again every few seconds while shown. */
export function useAdbDevices(adb: AdbStatus | null): {
  devices: Device[];
  error: string | null;
} {
  const [devices, setDevices] = useState<Device[]>([]);
  const [error, setError] = useState<string | null>(null);
  const path = adb?.path;
  useEffect(() => {
    if (!path) return;
    let stopped = false;
    const poll = () =>
      void api
        .androidDevices()
        .then((list) => {
          if (stopped) return;
          setDevices(list);
          setError(null);
        })
        .catch((e) => !stopped && setError(errorText(e)));
    poll();
    const timer = window.setInterval(poll, DEVICE_POLL_MS);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [path]);
  return { devices, error };
}

/** A phone's name: its model, or its address without the mDNS tail. */
export function deviceName(device: Device): string {
  return device.model ?? device.serial.replace(/\._adb-tls-connect\._tcp\.?$/, '');
}
