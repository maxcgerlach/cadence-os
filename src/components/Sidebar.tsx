import { open } from "@tauri-apps/plugin-dialog";
import { FolderPlus, ListMusic, Tags, Loader2 } from "lucide-react";
import { useLibrary } from "../context/LibraryContext";
import { useScanDirectory } from "../hooks/useCadence";

export function Sidebar() {
  const { view, setView } = useLibrary();
  const scanDirectory = useScanDirectory();

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

      {scanDirectory.data && (
        <p className="sidebar__scan-result">
          Indexed {scanDirectory.data.inserted} of {scanDirectory.data.scanned} found
        </p>
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
