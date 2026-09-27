import { useState } from "react";
import "./App.css";
import { Sidebar } from "./components/Sidebar";
import { SearchBar } from "./components/SearchBar";
import { SampleTable } from "./components/SampleTable";
import { SamplePreview } from "./components/SamplePreview";
import { CreateTagForm } from "./components/CreateTagForm";
import { TagSampleList } from "./components/TagSampleList";
import { useLibrary } from "./context/LibraryContext";
import { useSamples, useVirtualTags } from "./hooks/useCadence";
import { useLiveSampleUpdates } from "./hooks/useLiveSampleUpdates";

function AllSamplesView() {
  const { search, selectedSampleId, setSelectedSampleId } = useLibrary();
  const { data: samples = [], isLoading } = useSamples(search);

  const selectedSample = samples.find((s) => s.id === selectedSampleId) ?? null;

  return (
    <>
      <SearchBar />
      <SampleTable
        samples={samples}
        isLoading={isLoading}
        selectedSampleId={selectedSampleId}
        onSelect={(sample) => setSelectedSampleId(sample.id)}
      />
      <SamplePreview sample={selectedSample} />
    </>
  );
}

function VirtualTagsView() {
  const { data: tags = [], isLoading: tagsLoading } = useVirtualTags();
  const { data: samples = [] } = useSamples("");
  const [selectedTagId, setSelectedTagId] = useState<number | null>(null);

  const selectedTag = tags.find((t) => t.id === selectedTagId) ?? null;

  return (
    <div className="tags-view">
      <CreateTagForm />

      {tagsLoading ? (
        <p className="sample-table__empty">Loading tags...</p>
      ) : tags.length === 0 ? (
        <p className="sample-table__empty">No tags yet. Create one above.</p>
      ) : (
        <div className="tag-chip-row">
          {tags.map((tag) => (
            <button
              key={tag.id}
              className={tag.id === selectedTagId ? "tag-chip active" : "tag-chip"}
              style={{ borderColor: tag.color_hex }}
              onClick={() => setSelectedTagId(tag.id === selectedTagId ? null : tag.id)}
            >
              <span className="tag-chip__dot" style={{ backgroundColor: tag.color_hex }} />
              {tag.name}
            </button>
          ))}
        </div>
      )}

      {selectedTag && (
        <>
          <p className="tags-view__hint">
            Check samples to tag them as "{selectedTag.name}"
          </p>
          <TagSampleList samples={samples} tagId={selectedTag.id} />
        </>
      )}
    </div>
  );
}

function App() {
  const { view } = useLibrary();
  useLiveSampleUpdates();

  return (
    <div className="app-shell">
      <Sidebar />
      <main className="main-panel">
        {view === "all-samples" ? <AllSamplesView /> : <VirtualTagsView />}
      </main>
    </div>
  );
}

export default App;
