import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

# 1. Update Context Form State & `save` & `cancel`
# Wait, `chunkEndpointId` already exists as a global state! 
# Let's just reuse it as the form state instead of completely refactoring its declaration.
# But we need to load it in `startEdit` and reset it in `cancel`.

old_start_edit = """  const startEdit = (c: Context) => {
    setAdding(true);
    setEditingId(c.id);
    setName(c.name);
    setDescription(c.description ?? "");
    setChunkingStrategy(c.chunking_strategy);
    setProfileId(c.chunking_profile_id);
    setStructuralProfileId(c.structural_profile_id);
    setModelId(c.embedding_model_id);
  };"""

new_start_edit = """  const startEdit = (c: Context) => {
    setAdding(true);
    setEditingId(c.id);
    setName(c.name);
    setDescription(c.description ?? "");
    setChunkingStrategy(c.chunking_strategy);
    setProfileId(c.chunking_profile_id);
    setStructuralProfileId(c.structural_profile_id);
    setModelId(c.embedding_model_id);
    setChunkEndpointId(c.chunk_endpoint_id);
  };"""
text = text.replace(old_start_edit, new_start_edit)

# Update save()
old_save = """        chunking_strategy: chunkingStrategy,
        chunking_profile_id: profileId,
        structural_profile_id: structuralProfileId,
        embedding_model_id: modelId,
        embedding_dim: dim,
      };"""
new_save = """        chunking_strategy: chunkingStrategy,
        chunking_profile_id: profileId,
        structural_profile_id: structuralProfileId,
        embedding_model_id: modelId,
        embedding_dim: dim,
        chunk_endpoint_id: chunkEndpointId,
      };"""
text = text.replace(old_save, new_save)

# Update cancel() to reset chunkEndpointId
old_cancel = """  const cancel = () => {
    setAdding(false);
    setEditingId(null);
    setName("");
    setDescription("");
    setChunkingStrategy("structural");
    reload(); // Re-apply defaults
  };"""
new_cancel = """  const cancel = () => {
    setAdding(false);
    setEditingId(null);
    setName("");
    setDescription("");
    setChunkingStrategy("structural");
    reload(); // Re-apply defaults
    setChunkEndpointId(endpoints[0]?.id ?? null);
  };"""
text = text.replace(old_cancel, new_cancel)

# Move the UI from ctx-head into the form
# Remove ctx-head block
old_ctx_head = """      <div className="ctx-head">
        <label className="inline">Chunk with:
          <select value={chunkEndpointId ?? ""}
            onChange={(e) => pickChunkEndpoint(e.target.value ? Number(e.target.value) : null)}>
            {endpoints.length === 0 && <option value="">— kein LLM-Endpoint —</option>}
            {endpoints.map((ep) => <option key={ep.id} value={ep.id}>{ep.name}</option>)}
          </select>
        </label>
        <span className="fill" />
        <button className="primary" onClick={() => { setAdding(!adding); setEditingId(null); }}>
          {adding && editingId == null ? "✕ Abbrechen" : "➕ Kontext anlegen"}
        </button>
      </div>"""
new_ctx_head = """      <div className="ctx-head">
        <span className="fill" />
        <button className="primary" onClick={() => { setAdding(!adding); setEditingId(null); }}>
          {adding && editingId == null ? "✕ Abbrechen" : "➕ Kontext anlegen"}
        </button>
      </div>"""
text = text.replace(old_ctx_head, new_ctx_head)

# Also we need to remove `pickChunkEndpoint` and just use setChunkEndpointId directly in the select
old_pick = """  const pickChunkEndpoint = (id: number | null) => {
    setChunkEndpointId(id);
    if (id != null) void api.setSetting("chunk_endpoint_id", id);
  };"""
text = text.replace(old_pick, "")

# Insert the select into the form
old_form_profile = """      {chunkingStrategy === "prompt" && (
        <select value={profileId ?? ""} onChange={(e) => setProfileId(e.target.value ? Number(e.target.value) : null)}>
          <option value="">— chunking profile —</option>
          {profiles.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
        </select>
      )}"""
new_form_profile = """      {chunkingStrategy === "prompt" && (
        <>
          <select value={profileId ?? ""} onChange={(e) => setProfileId(e.target.value ? Number(e.target.value) : null)}>
            <option value="">— chunking profile —</option>
            {profiles.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
          </select>
          <select value={chunkEndpointId ?? ""} onChange={(e) => setChunkEndpointId(e.target.value ? Number(e.target.value) : null)}>
            <option value="">— LLM endpoint (for chunks) —</option>
            {endpoints.map((ep) => <option key={ep.id} value={ep.id}>{ep.name}</option>)}
          </select>
        </>
      )}"""
text = text.replace(old_form_profile, new_form_profile)

# Finally, update the `runChunking` calls to use `ctx.chunk_endpoint_id` instead of the global `chunkEndpointId` state
# First usage: `chunking` function
old_chunking_check = """    const ctx = contexts.find(c => c.id === contextId);
    const isStructural = ctx?.chunking_strategy === "structural";

    if (!isStructural && chunkEndpointId == null) {
      setError("Bitte oben bei 'Chunk with' einen LLM-Endpoint wählen.");
      return;
    }"""
new_chunking_check = """    const ctx = contexts.find(c => c.id === contextId);
    const isStructural = ctx?.chunking_strategy === "structural";

    if (!isStructural && ctx?.chunk_endpoint_id == null) {
      setError("Diesem Kontext ist noch kein LLM-Endpoint zum Chunken zugewiesen. Bitte bearbeiten (✏️) und einen Endpunkt auswählen.");
      return;
    }"""
text = text.replace(old_chunking_check, new_chunking_check)

old_run_chunking_1 = "await api.runChunking(contextId, d.id, d.extracted_text ?? \"\", chunkEndpointId!);"
new_run_chunking_1 = "await api.runChunking(contextId, d.id, d.extracted_text ?? \"\", ctx!.chunk_endpoint_id!);"
text = text.replace(old_run_chunking_1, new_run_chunking_1)

old_run_chunking_2 = "const total = await api.runChunking(contextId, doc.id, pasteText, chunkEndpointId!);"
new_run_chunking_2 = "const total = await api.runChunking(contextId, doc.id, pasteText, ctx!.chunk_endpoint_id!);"
text = text.replace(old_run_chunking_2, new_run_chunking_2)

# Fix the paste UI:
old_paste_ui = '<span className="muted">Chunk with: {endpoints.find((e) => e.id === chunkEndpointId)?.name ?? "—"}</span>'
new_paste_ui = '<span className="muted">Chunk with: {endpoints.find((e) => e.id === c.chunk_endpoint_id)?.name ?? "—"}</span>'
text = text.replace(old_paste_ui, new_paste_ui)

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)

print("pages.tsx patched")
