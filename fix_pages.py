import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    content = f.read()

# Replace the broken overlay-head section with the correct one that includes the buttons
correct_header = """
        <div className="card">
          <div className="row" style={{ alignItems: "center" }}>
            <b>Kontext {viewerId}</b>
            <button className={viewerMode === "chunks" ? "" : "link"}
              onClick={() => void viewChunks(viewerId)}>Chunks</button>
            <button className={viewerMode === "prechunks" ? "" : "link"}
              onClick={() => void viewPrechunks(viewerId)}>LLM-Antworten</button>
            <button className="link"
              onClick={() => void (viewerMode === "chunks" ? viewChunks(viewerId) : viewPrechunks(viewerId))}>
              ↻
            </button>
            <div style={{ flex: 1 }} />
            <span className="muted" style={{ display: "flex", gap: "8px", alignItems: "center" }}>
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
            <button className="link" onClick={() => setViewerId(null)}>schließen</button>
          </div>
"""

# We need to find where the `        <div className="card">` block is under `if (viewerId != null) {`
pattern = re.compile(r'\s*<div className="card">\s*<div className="overlay-head".*?</div>\s*</div>', re.DOTALL)
new_content = pattern.sub(correct_header, content, count=1)

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(new_content)
print("done")
