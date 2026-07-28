import { useState } from "react";
import { useCreateTag } from "../hooks/useCadence";

const DEFAULT_COLOR = "#6366f1";

export function CreateTagForm() {
  const [name, setName] = useState("");
  const [colorHex, setColorHex] = useState(DEFAULT_COLOR);
  const createTag = useCreateTag();

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (name.trim() === "") return;
    createTag.mutate(
      { name, colorHex },
      {
        onSuccess: () => {
          setName("");
          setColorHex(DEFAULT_COLOR);
        },
      },
    );
  }

  return (
    <form className="create-tag-form" onSubmit={handleSubmit}>
      <input
        type="color"
        value={colorHex}
        onChange={(e) => setColorHex(e.target.value)}
        title="Tag color"
      />
      <input
        type="text"
        placeholder="New tag name..."
        value={name}
        onChange={(e) => setName(e.target.value)}
      />
      <button type="submit" disabled={createTag.isPending || name.trim() === ""}>
        Add Tag
      </button>
      {createTag.isError && (
        <p className="sidebar__scan-error">{String(createTag.error)}</p>
      )}
    </form>
  );
}
