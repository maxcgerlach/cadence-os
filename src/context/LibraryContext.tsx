import { createContext, useContext, useMemo, useState, type ReactNode } from "react";

export type LibraryView = "all-samples" | "virtual-tags";

interface LibraryContextValue {
  view: LibraryView;
  setView: (view: LibraryView) => void;
  search: string;
  setSearch: (search: string) => void;
  selectedSampleId: number | null;
  setSelectedSampleId: (id: number | null) => void;
}

const LibraryContext = createContext<LibraryContextValue | null>(null);

export function LibraryProvider({ children }: { children: ReactNode }) {
  const [view, setView] = useState<LibraryView>("all-samples");
  const [search, setSearch] = useState("");
  const [selectedSampleId, setSelectedSampleId] = useState<number | null>(null);

  const value = useMemo(
    () => ({ view, setView, search, setSearch, selectedSampleId, setSelectedSampleId }),
    [view, search, selectedSampleId],
  );

  return <LibraryContext.Provider value={value}>{children}</LibraryContext.Provider>;
}

export function useLibrary() {
  const ctx = useContext(LibraryContext);
  if (!ctx) throw new Error("useLibrary must be used within a LibraryProvider");
  return ctx;
}
