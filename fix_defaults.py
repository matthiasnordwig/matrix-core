import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

# Change default chunkingStrategy
text = text.replace('const [chunkingStrategy, setChunkingStrategy] = useState("prompt");', 'const [chunkingStrategy, setChunkingStrategy] = useState("structural");')

old_reload = """  const reload = () => {
    void api.listContexts().then(setContexts).catch((e) => setError(reportError(e)));
    void api.listChunkingProfiles().then(setProfiles).catch(() => {});
    void api.listStructuralProfiles().then(setStructuralProfiles).catch(() => {});
    void api.listEmbeddingModels().then(setModels).catch(() => {});
    void api.listLlmEndpoints().then(setEndpoints).catch(() => {});
  };"""

new_reload = """  const reload = () => {
    void api.listContexts().then(setContexts).catch((e) => setError(reportError(e)));
    void api.listChunkingProfiles().then((p) => {
      setProfiles(p);
      if (p.length > 0) setProfileId(prev => prev ?? p[0].id);
    }).catch(() => {});
    void api.listStructuralProfiles().then((p) => {
      setStructuralProfiles(p);
      const def = p.find(x => x.name.toLowerCase() === "default" || x.name.toLowerCase() === "standard");
      if (def) setStructuralProfileId(prev => prev ?? def.id);
      else if (p.length > 0) setStructuralProfileId(prev => prev ?? p[0].id);
    }).catch(() => {});
    void api.listEmbeddingModels().then((m) => {
      setModels(m);
      const jina = m.find(x => x.identifier.toLowerCase().includes("jina"));
      if (jina) setModelId(prev => prev ?? jina.id);
      else if (m.length > 0) setModelId(prev => prev ?? m[0].id);
    }).catch(() => {});
    void api.listLlmEndpoints().then(setEndpoints).catch(() => {});
  };"""

text = text.replace(old_reload, new_reload)

# Make sure the cancel/reset behavior resets to these defaults as well
old_cancel = """  const cancel = () => {
    setAdding(false);
    setEditingId(null);
    setName("");
    setDescription("");
  };"""

new_cancel = """  const cancel = () => {
    setAdding(false);
    setEditingId(null);
    setName("");
    setDescription("");
    setChunkingStrategy("structural");
    reload(); // Re-apply defaults
  };"""

text = text.replace(old_cancel, new_cancel)

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)

print("done")
