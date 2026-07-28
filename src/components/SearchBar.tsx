import { Search } from "lucide-react";
import { useLibrary } from "../context/LibraryContext";

export function SearchBar() {
  const { search, setSearch } = useLibrary();

  return (
    <div className="search-bar">
      <Search size={16} />
      <input
        type="text"
        placeholder="Filter samples by name..."
        value={search}
        onChange={(e) => setSearch(e.target.value)}
      />
    </div>
  );
}
