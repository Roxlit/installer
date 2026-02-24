import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { RoxlitConfig, UpdateInfo } from "@/lib/types";

export function useUpdateChecker(config: RoxlitConfig | null) {
  const [update, setUpdate] = useState<UpdateInfo | null>(null);

  const checkForUpdate = useCallback(async () => {
    if (!config) return;

    try {
      const result = await invoke<UpdateInfo | null>("check_for_update", {
        lastCheck: config.lastUpdateCheck ?? null,
        dismissedVersion: config.dismissedVersion ?? null,
      });

      // Persist the check timestamp
      const now = new Date().toISOString().replace(/\.\d{3}Z$/, "Z");
      await invoke("save_update_state", {
        lastUpdateCheck: now,
        dismissedVersion: null,
      });

      setUpdate(result);
    } catch {
      // Silent failure — update check is non-critical
    }
  }, [config]);

  useEffect(() => {
    checkForUpdate();
  }, [checkForUpdate]);

  const dismissUpdate = useCallback(async () => {
    if (!update) return;
    try {
      await invoke("save_update_state", {
        lastUpdateCheck: null,
        dismissedVersion: update.version,
      });
    } catch {
      // Silent failure
    }
    setUpdate(null);
  }, [update]);

  return { update, dismissUpdate };
}
