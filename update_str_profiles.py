import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

str_form_old = """          <div className="card form">
            <input placeholder="Profile name" value={strName} onChange={(e) => setStrName(e.target.value)} />
            
            <div className="row">
              <label>Min Chunk Size
                <input type="number" step="100" min="0" value={strMinSize} onChange={(e) => setStrMinSize(Number(e.target.value))} />
              </label>
              <label>Max Chunk Size
                <input type="number" step="100" min="500" value={strMaxSize} onChange={(e) => setStrMaxSize(Number(e.target.value))} />
              </label>
            </div>

            <h4>Regex Patterns</h4>
            {strPatterns.map((p, i) => (
              <div key={i} className="row" style={{ alignItems: "center", background: "#f5f5f5", padding: "8px", borderRadius: "4px" }}>
                <input placeholder="Group/Label" value={p.group_name} onChange={(e) => {
                  const newP = [...strPatterns]; newP[i].group_name = e.target.value; setStrPatterns(newP);
                }} style={{ width: "120px" }} />
                <select value={p.role} onChange={(e) => {
                  const newP = [...strPatterns]; newP[i].role = e.target.value; setStrPatterns(newP);
                }}>
                  <option value="heading_l1">Heading</option>
                  <option value="definition">Definition</option>
                  <option value="ignore">Ignore</option>
                </select>
                <input placeholder="Regex" value={p.regex} onChange={(e) => {
                  const newP = [...strPatterns]; newP[i].regex = e.target.value; setStrPatterns(newP);
                }} style={{ flex: 1 }} />
                <input placeholder="Flags (i, m)" value={p.flags} onChange={(e) => {
                  const newP = [...strPatterns]; newP[i].flags = e.target.value; setStrPatterns(newP);
                }} style={{ width: "60px" }} />
                <input type="number" title="Priority" value={p.priority} onChange={(e) => {
                  const newP = [...strPatterns]; newP[i].priority = Number(e.target.value); setStrPatterns(newP);
                }} style={{ width: "70px" }} />
                <button className="danger" onClick={() => setStrPatterns(strPatterns.filter((_, idx) => idx !== i))}>🗑</button>
              </div>
            ))}
            <div>
              <button className="link" onClick={() => setStrPatterns([...strPatterns, {
                group_name: "Neue Regel", role: "heading_l1", regex: "", flags: "i", priority: 100, label: null, sort_order: strPatterns.length
              }])}>+ Pattern hinzufügen</button>
            </div>

            <div className="row" style={{ marginTop: "1rem" }}>
              <button disabled={!strName} onClick={() => void saveStr()}>
                {editingStrId != null ? "Profil speichern" : "Profil anlegen"}
              </button>
              {editingStrId != null && <button className="link" onClick={cancelStr}>abbrechen</button>}
            </div>
          </div>"""

# Remove the old form rendering
text = text.replace(str_form_old, "")

str_form_jsx = """          <div className="form">
            <input placeholder="Profile name" value={strName} onChange={(e) => setStrName(e.target.value)} />
            
            <div className="row">
              <label>Min Chunk Size
                <input type="number" step="100" min="0" value={strMinSize} onChange={(e) => setStrMinSize(Number(e.target.value))} />
              </label>
              <label>Max Chunk Size
                <input type="number" step="100" min="500" value={strMaxSize} onChange={(e) => setStrMaxSize(Number(e.target.value))} />
              </label>
            </div>

            <h4>Regex Patterns</h4>
            {strPatterns.map((p, i) => (
              <div key={i} className="row" style={{ alignItems: "center", background: "#f5f5f5", padding: "8px", borderRadius: "4px" }}>
                <input placeholder="Group/Label" value={p.group_name} onChange={(e) => {
                  const newP = [...strPatterns]; newP[i].group_name = e.target.value; setStrPatterns(newP);
                }} style={{ width: "120px" }} />
                <select value={p.role} onChange={(e) => {
                  const newP = [...strPatterns]; newP[i].role = e.target.value; setStrPatterns(newP);
                }}>
                  <option value="heading_l1">Heading</option>
                  <option value="definition">Definition</option>
                  <option value="ignore">Ignore</option>
                </select>
                <input placeholder="Regex" value={p.regex} onChange={(e) => {
                  const newP = [...strPatterns]; newP[i].regex = e.target.value; setStrPatterns(newP);
                }} style={{ flex: 1 }} />
                <input placeholder="Flags (i, m)" value={p.flags} onChange={(e) => {
                  const newP = [...strPatterns]; newP[i].flags = e.target.value; setStrPatterns(newP);
                }} style={{ width: "60px" }} />
                <input type="number" title="Priority" value={p.priority} onChange={(e) => {
                  const newP = [...strPatterns]; newP[i].priority = Number(e.target.value); setStrPatterns(newP);
                }} style={{ width: "70px" }} />
                <button className="danger" onClick={() => setStrPatterns(strPatterns.filter((_, idx) => idx !== i))}>🗑</button>
              </div>
            ))}
            <div>
              <button className="link" onClick={() => setStrPatterns([...strPatterns, {
                group_name: "Neue Regel", role: "heading_l1", regex: "", flags: "i", priority: 100, label: null, sort_order: strPatterns.length
              }])}>+ Pattern hinzufügen</button>
            </div>

            <div className="row" style={{ marginTop: "1rem" }}>
              <button disabled={!strName} onClick={() => void saveStr()}>
                {editingStrId != null ? "Profil speichern" : "Profil anlegen"}
              </button>
              <button className="link" onClick={cancelStr}>Cancel</button>
            </div>
          </div>"""

str_ul_old = """          <ul className="list">
            {structuralProfiles.map((p) => (
              <li key={p.id}>
                <b>{p.name}</b> · {p.min_chunk_chars}-{p.max_chunk_chars} chars · {p.patterns.length} pattern(s)
                <button className="link" onClick={() => startEditStr(p)}>edit</button>
                {confirmDeleteStrId === p.id ? (
                  <span className="confirm" style={{ marginLeft: "10px" }}>
                    <button className="icon-btn danger" title="Confirm delete" onClick={() => void api.deleteStructuralProfile({ id: p.id }).then(reload)}>✓ Delete</button>
                    <button className="cancel-btn" onClick={() => setConfirmDeleteStrId(null)}>Cancel</button>
                  </span>
                ) : (
                  <button className="link" onClick={() => setConfirmDeleteStrId(p.id)}>delete</button>
                )}
              </li>
            ))}
          </ul>"""

str_ul_new = """          <table className="grid">
            <thead>
              <tr><th>Name</th><th>Config</th><th>Actions</th></tr>
            </thead>
            <tbody>
              {structuralProfiles.map((p) => (
                <Fragment key={p.id}>
                  <tr>
                    <td>{p.name}</td>
                    <td>{p.min_chunk_chars}-{p.max_chunk_chars} chars, {p.patterns.length} pattern(s)</td>
                    <td className="col-actions">
                      {confirmDeleteStrId === p.id ? (
                        <span className="confirm">
                          Delete?
                          <button className="icon-btn danger" title="Confirm delete" onClick={() => void api.deleteStructuralProfile({ id: p.id }).then(() => { setConfirmDeleteStrId(null); reload(); })}>✓ Delete</button>
                          <button className="cancel-btn" onClick={() => setConfirmDeleteStrId(null)}>Cancel</button>
                        </span>
                      ) : (
                        <div className="actions-menu">
                          <button className="menu-trigger" title="Actions" onClick={() => setMenuForStr(menuForStr === p.id ? null : p.id)}>⋮</button>
                          {menuForStr === p.id && (
                            <div className="menu-dropdown">
                              <button onClick={() => { setMenuForStr(null); startEditStr(p); }}>✏️ Edit profile</button>
                              <button onClick={() => { setMenuForStr(null); setConfirmDeleteStrId(p.id); }}>🗑️ Delete profile</button>
                            </div>
                          )}
                        </div>
                      )}
                    </td>
                  </tr>
                  {editingStrId === p.id && (
                    <tr>
                      <td colSpan={3}>
                        {str_form_jsx}
                      </td>
                    </tr>
                  )}
                </Fragment>
              ))}
            </tbody>
          </table>
          {editingStrId == null && (
            <div className="card" style={{ marginTop: "2rem" }}>
              <h3>Add Structural Profile</h3>
              {str_form_jsx}
            </div>
          )}""".replace("{str_form_jsx}", str_form_jsx)

text = text.replace(str_ul_old, str_ul_new)

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)

print("Structural Profiles patched.")
