import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

# 1. Remove duplicate top-k lines
# Line 1024 duplicate
text = text.replace('          <label style={{display:"flex", alignItems:"center", gap:"8px"}}>top-k <input type="number" min="1" max="20" value={topK} onChange={(e) => setTopK(Number(e.target.value))} style={{width:"60px", padding:"4px"}} /></label>\n', '', 1)
# Line 1119 duplicate
text = text.replace('          <label style={{display:"flex", alignItems:"center", gap:"8px"}}>top-k <input type="number" min="1" max="20" value={topK} onChange={(e) => setTopK(Number(e.target.value))} style={{width:"60px", padding:"4px"}} /></label>\n', '', 1)

# 2. Fix the "➕ Kontext anlegen" button and cancel/defaults
old_button = """        <button className="primary" onClick={() => { setAdding(!adding); setEditingId(null); }}>
          {adding && editingId == null ? "✕ Abbrechen" : "➕ Kontext anlegen"}
        </button>"""

new_button = """        <button className="primary" onClick={() => {
          if (adding && editingId == null) {
            cancel();
          } else {
            startAdd();
          }
        }}>
          {adding && editingId == null ? "✕ Abbrechen" : "➕ Kontext anlegen"}
        </button>"""
text = text.replace(old_button, new_button)

old_start_edit = """  const startEdit = (c: Context) => {"""
new_start_edit = """  const startAdd = () => {
    setAdding(true);
    setEditingId(null);
    setName("");
    setDescription("");
    setChunkingStrategy("structural");
    
    // Set chunking profile
    if (profiles.length > 0) setProfileId(profiles[0].id);
    
    // Set structural profile (default/standard)
    const defStruct = structuralProfiles.find(x => x.name.toLowerCase() === "default" || x.name.toLowerCase() === "standard");
    if (defStruct) setStructuralProfileId(defStruct.id);
    else if (structuralProfiles.length > 0) setStructuralProfileId(structuralProfiles[0].id);
    
    // Set embedding model
    const jina = models.find(x => x.identifier.toLowerCase().includes("jina"));
    if (jina) setModelId(jina.id);
    else if (models.length > 0) setModelId(models[0].id);
    
    // Set endpoint
    setChunkEndpointId(endpoints[0]?.id ?? null);
  };

  const startEdit = (c: Context) => {"""
text = text.replace(old_start_edit, new_start_edit)

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)

print("UI bugs fixed")
