import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

# We know llm_form_jsx is already in my previous script, but wait! The previous script failed to replace llm_form_old too!
# Let's check if the LLM form is still the old one.
pattern_form = re.compile(r'          <div className="card form">\s*<input placeholder="Profile name".*?</div>\s*</div>', re.DOTALL)

llm_form_jsx = """          <div className="form">
            <input placeholder="Profile name" value={name} onChange={(e) => setName(e.target.value)} />
            <div className="row">
              <label>Overlap ratio
                <input type="number" step="0.05" min="0" max="0.5" value={overlap} onChange={(e) => setOverlap(Number(e.target.value))} />
              </label>
              <label>LLM Endpoint
                <select value={endpointId ?? ""} onChange={(e) => setEndpointId(e.target.value ? Number(e.target.value) : null)}>
                  {endpoints.map((ep) => <option key={ep.id} value={ep.id}>{ep.name}</option>)}
                </select>
              </label>
            </div>
            <div className="row">
              <button disabled={!name || endpointId == null} onClick={() => void saveLlm()}>
                {editingId != null ? "Profil speichern" : "Profil anlegen"}
              </button>
              <button className="link" onClick={cancelLlm}>Cancel</button>
            </div>
          </div>"""

# Remove the old form (it's sitting above the list)
text = pattern_form.sub("", text, count=1)

# Now replace the <ul className="list"> for LLM profiles
pattern_ul = re.compile(r'          <ul className="list">\s*\{profiles\.map\(\(p\) => \(\s*<li key=\{p\.id\}>.*?</li>\s*\)\)\}\s*</ul>', re.DOTALL)

llm_ul_new = """          <table className="grid">
            <thead>
              <tr><th>Name</th><th>Config</th><th>Actions</th></tr>
            </thead>
            <tbody>
              {profiles.map((p) => (
                <Fragment key={p.id}>
                  <tr>
                    <td>{p.name}</td>
                    <td>{p.overlap_ratio * 100}% overlap, endpoint {p.llm_endpoint_id}</td>
                    <td className="col-actions">
                      {confirmDeleteLlmId === p.id ? (
                        <span className="confirm">
                          Delete?
                          <button className="icon-btn danger" title="Confirm delete" onClick={() => void api.deleteChunkingProfile({ id: p.id }).then(() => { setConfirmDeleteLlmId(null); reload(); })}>✓ Delete</button>
                          <button className="cancel-btn" onClick={() => setConfirmDeleteLlmId(null)}>Cancel</button>
                        </span>
                      ) : (
                        <div className="actions-menu">
                          <button className="menu-trigger" title="Actions" onClick={() => setMenuForLlm(menuForLlm === p.id ? null : p.id)}>⋮</button>
                          {menuForLlm === p.id && (
                            <div className="menu-dropdown">
                              <button onClick={() => { setMenuForLlm(null); startEditLlm(p); }}>✏️ Edit profile</button>
                              <button onClick={() => { setMenuForLlm(null); setConfirmDeleteLlmId(p.id); }}>🗑️ Delete profile</button>
                            </div>
                          )}
                        </div>
                      )}
                    </td>
                  </tr>
                  {editingId === p.id && (
                    <tr>
                      <td colSpan={3}>
                        {llm_form_jsx}
                      </td>
                    </tr>
                  )}
                </Fragment>
              ))}
            </tbody>
          </table>
          {editingId == null && (
            <div className="card" style={{ marginTop: "2rem" }}>
              <h3>Add LLM Profile</h3>
              {llm_form_jsx}
            </div>
          )}""".replace("{llm_form_jsx}", llm_form_jsx)

text = pattern_ul.sub(llm_ul_new, text, count=1)

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)

print("done")
