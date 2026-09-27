import { useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";

/**
 * Subscribes to the "library-updated" event the Rust filesystem watcher
 * emits whenever a watched folder changes (new/removed samples or FL
 * Studio project files), and refetches so the UI stays in sync without a
 * manual re-scan.
 */
export function useLiveSampleUpdates() {
  const queryClient = useQueryClient();

  useEffect(() => {
    const unlisten = listen("library-updated", () => {
      queryClient.invalidateQueries({ queryKey: ["samples"] });
      queryClient.invalidateQueries({ queryKey: ["projects"] });
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [queryClient]);
}
