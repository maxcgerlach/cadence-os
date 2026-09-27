import { open } from "@tauri-apps/plugin-dialog";
import { FolderPlus, ListMusic, Tags, Loader2 } from "lucide-react";
import { useLibrary } from "../context/LibraryContext";
import { useScanDirectory, useSamples } from "../hooks/useCadence";

export function Sidebar() {
  const { view, setView } = useLibrary();
  const scanDirectory = useScanDirectory();
  // Derived from the live samples query rather than the last scan's result,
  // so it stays accurate when the filesystem watcher re-scans in the
  // background (a manual click isn't the only thing that can change it).
  const { data: samples = [] } = useSamples("");

  async function handleAddFolder() {
    const dirPath = await open({ directory: true, multiple: false });
    if (typeof dirPath === "string") {
      scanDirectory.mutate(dirPath);
    }
  }

  return (
    <aside className="sidebar">
      <button
        className="sidebar__add-folder"
        onClick={handleAddFolder}
        disabled={scanDirectory.isPending}
      >
        {scanDirectory.isPending ? (
          <Loader2 size={16} className="spin" />
        ) : (
          <FolderPlus size={16} />
        )}
        Add Folder to Index
      </button>

      {samples.length > 0 && (
        <p className="sidebar__scan-result">{samples.length} samples indexed</p>
      )}
      {scanDirectory.isError && (
        <p className="sidebar__scan-error">{String(scanDirectory.error)}</p>
      )}

      <nav className="sidebar__nav">
        <button
          className={view === "all-samples" ? "sidebar__nav-item active" : "sidebar__nav-item"}
          onClick={() => setView("all-samples")}
        >
          <ListMusic size={16} />
          All Samples
        </button>
        <button
          className={view === "virtual-tags" ? "sidebar__nav-item active" : "sidebar__nav-item"}
          onClick={() => setView("virtual-tags")}
        >
          <Tags size={16} />
          Virtual Tags
        </button>
      </nav>
    </aside>
  );
}
