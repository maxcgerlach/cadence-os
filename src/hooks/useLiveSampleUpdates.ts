import { useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";

/**
 * Subscribes to the "samples-updated" event the Rust filesystem watcher
 * emits whenever a watched folder changes, and refetches the samples list
 * so the UI stays in sync without a manual re-scan.
 */
export function useLiveSampleUpdates() {
  const queryClient = useQueryClient();

  useEffect(() => {
    const unlisten = listen("samples-updated", () => {
      queryClient.invalidateQueries({ queryKey: ["samples"] });
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [queryClient]);
}
