import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

# 1. Add state hooks
hook_str = """  const [viewerPrechunks, setViewerPrechunks] = useState<Prechunk[]>([]);
  const [viewerChunkPage, setViewerChunkPage] = useState<number>(0);
  const CHUNKS_PER_PAGE = 200;"""
text = text.replace('  const [viewerPrechunks, setViewerPrechunks] = useState<Prechunk[]>([]);', hook_str)

# 2. Add resets in viewChunks
vc_str = """  const viewChunks = async (id: number) => {
    setViewerId(id);
    setViewerMode("chunks");
    setViewerChunkPage(0);"""
text = text.replace('  const viewChunks = async (id: number) => {\n    setViewerId(id);\n    setViewerMode("chunks");', vc_str)

vp_str = """  const viewPrechunks = async (id: number) => {
    setViewerId(id);
    setViewerMode("prechunks");
    setViewerChunkPage(0);"""
text = text.replace('  const viewPrechunks = async (id: number) => {\n    setViewerId(id);\n    setViewerMode("prechunks");', vp_str)

# 3. Replace the text displaying "200 von 500 Chunks" with the new string + pagination buttons
old_display = """            <span className="muted">
              {viewerMode === "chunks"
                ? `${Math.min(viewerChunks.length, 200)} von ${viewerChunks.length} Chunks`
                : `${viewerPrechunks.length} Prechunks`}
            </span>
            <button className="link" onClick={() => setViewerId(null)}>schließen</button>"""

new_display = """            <span className="muted" style={{ display: "flex", gap: "8px", alignItems: "center" }}>
              {viewerMode === "chunks" ? (
                viewerChunks.length === 0 ? "0 Chunks" :
                `Chunks ${viewerChunkPage * CHUNKS_PER_PAGE + 1}–${Math.min((viewerChunkPage + 1) * CHUNKS_PER_PAGE, viewerChunks.length)} von ${viewerChunks.length}`
              ) : (
                viewerPrechunks.length === 0 ? "0 Prechunks" :
                `Prechunks ${viewerChunkPage * CHUNKS_PER_PAGE + 1}–${Math.min((viewerChunkPage + 1) * CHUNKS_PER_PAGE, viewerPrechunks.length)} von ${viewerPrechunks.length}`
              )}
              
              <button 
                className="icon-btn" 
                disabled={viewerChunkPage === 0} 
                onClick={() => setViewerChunkPage(p => p - 1)}
                title="Vorherige Seite"
                style={{ padding: "2px 8px", marginLeft: "8px" }}
              >
                &larr;
              </button>
              <button 
                className="icon-btn" 
                disabled={(viewerChunkPage + 1) * CHUNKS_PER_PAGE >= (viewerMode === "chunks" ? viewerChunks.length : viewerPrechunks.length)} 
                onClick={() => setViewerChunkPage(p => p + 1)}
                title="Nächste Seite"
                style={{ padding: "2px 8px" }}
              >
                &rarr;
              </button>
            </span>
            <button className="link" onClick={() => setViewerId(null)}>schließen</button>"""

text = text.replace(old_display, new_display)

# 4. Replace .slice(0, 200) with .slice(...)
text = text.replace('{viewerChunks.slice(0, 200).map((c) => (', '{viewerChunks.slice(viewerChunkPage * CHUNKS_PER_PAGE, (viewerChunkPage + 1) * CHUNKS_PER_PAGE).map((c) => (')

# 5. Apply .slice to prechunks map
text = text.replace('{viewerPrechunks.map((p) => (', '{viewerPrechunks.slice(viewerChunkPage * CHUNKS_PER_PAGE, (viewerChunkPage + 1) * CHUNKS_PER_PAGE).map((p) => (')

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)

print("Patch applied.")
