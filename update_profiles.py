import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

state_hooks_old = """  const [editingStrId, setEditingStrId] = useState<number | null>(null);

  const [confirmDeleteLlmId, setConfirmDeleteLlmId] = useState<number | null>(null);
  const [confirmDeleteStrId, setConfirmDeleteStrId] = useState<number | null>(null);"""

state_hooks_new = """  const [editingStrId, setEditingStrId] = useState<number | null>(null);

  const [confirmDeleteLlmId, setConfirmDeleteLlmId] = useState<number | null>(null);
  const [confirmDeleteStrId, setConfirmDeleteStrId] = useState<number | null>(null);
  const [menuForLlm, setMenuForLlm] = useState<number | null>(null);
  const [menuForStr, setMenuForStr] = useState<number | null>(null);"""

text = text.replace(state_hooks_old, state_hooks_new)

# Locate the LLM form component to reuse it inline
llm_form_old = """          <div className="card form">
            <input placeholder="Profile name" value={name} onChange={(e) => setName(e.target.value)} />
            <textarea placeholder="Prompt (must contain {{pre_chunk}})" value={prompt} onChange={(e) => setPrompt(e.target.value)} />
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
              <button disabled={!name || !prompt || endpointId == null} onClick={() => void saveLlm()}>
                {editingId != null ? "Profil speichern" : "Profil anlegen"}
              </button>
              {editingId != null && <button className="link" onClick={cancelLlm}>abbrechen</button>}
            </div>
          </div>"""

# Replace it with an empty string since we will render it dynamically
text = text.replace(llm_form_old, "")

llm_form_jsx = """          <div className="form">
            <input placeholder="Profile name" value={name} onChange={(e) => setName(e.target.value)} />
            <textarea placeholder="Prompt (must contain {{pre_chunk}})" value={prompt} onChange={(e) => setPrompt(e.target.value)} />
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
              <button disabled={!name || !prompt || endpointId == null} onClick={() => void saveLlm()}>
                {editingId != null ? "Profil speichern" : "Profil anlegen"}
              </button>
              <button className="link" onClick={cancelLlm}>Cancel</button>
            </div>
          </div>"""

llm_ul_old = """          <ul className="list">
            {profiles.map((p) => (
              <li key={p.id}>
                <b>{p.name}</b> · {p.overlap_ratio * 100}% overlap · endpoint {p.llm_endpoint_id}
                <button className="link" onClick={() => startEditLlm(p)}>edit</button>
                {confirmDeleteLlmId === p.id ? (
                  <span className="confirm" style={{ marginLeft: "10px" }}>
                    <button className="icon-btn danger" title="Confirm delete" onClick={() => void api.deleteChunkingProfile({ id: p.id }).then(reload)}>✓ Delete</button>
                    <button className="cancel-btn" onClick={() => setConfirmDeleteLlmId(null)}>Cancel</button>
                  </span>
                ) : (
                  <button className="link" onClick={() => setConfirmDeleteLlmId(p.id)}>delete</button>
                )}
              </li>
            ))}
          </ul>"""

llm_ul_new = """          <table className="grid">
            <thead>
              <tr><th>Name</th><th>Model Endpoint ID</th><th>Actions</th></tr>
            </thead>
            <tbody>
              {profiles.map((p) => (
                <Fragment key={p.id}>
                  <tr>
                    <td>{p.name}</td>
                    <td>{p.llm_endpoint_id} ({p.overlap_ratio * 100}% overlap)</td>
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

text = text.replace(llm_ul_old, llm_ul_new)

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)

print("LLM Profiles patched.")
