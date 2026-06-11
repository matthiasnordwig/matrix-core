import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

# Replace LLM list
old_llm = """          <ul className="list">
            {profiles.map((p) => (
              <li key={p.id}>
                <b>{p.name}</b> · overlap {p.overlap_ratio} · sig≤{p.max_signature_len}
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

new_llm = """          <table className="grid">
            <thead>
              <tr><th>Name</th><th>Configuration</th><th>Actions</th></tr>
            </thead>
            <tbody>
              {profiles.map((p) => (
                <Fragment key={p.id}>
                  <tr>
                    <td>{p.name}</td>
                    <td>overlap {p.overlap_ratio} · sig≤{p.max_signature_len}</td>
                    <td className="col-actions">
                      {confirmDeleteLlmId === p.id ? (
                        <span className="confirm">
                          Delete?
                          <button className="icon-btn danger" title="Confirm delete" onClick={() => void api.deleteChunkingProfile({ id: p.id }).then(reload)}>✓</button>
                          <button className="cancel-btn" onClick={() => setConfirmDeleteLlmId(null)}>Cancel</button>
                        </span>
                      ) : (
                        <div className="actions-menu">
                          <button className="menu-trigger" title="Actions" onClick={() => setMenuForLlm(menuForLlm === p.id ? null : p.id)}>⋯</button>
                          {menuForLlm === p.id && (
                            <>
                              <div className="menu-backdrop" onClick={() => setMenuForLlm(null)} />
                              <div className="row-menu">
                                <button onClick={() => { setMenuForLlm(null); startEditLlm(p); }}>✏️ Edit profile</button>
                                <button onClick={() => { setMenuForLlm(null); setConfirmDeleteLlmId(p.id); }}>🗑️ Delete profile</button>
                              </div>
                            </>
                          )}
                        </div>
                      )}
                    </td>
                  </tr>
                </Fragment>
              ))}
              {profiles.length === 0 && <tr><td colSpan={3} className="muted" style={{textAlign:"center"}}>keine Profil-Vorlagen</td></tr>}
            </tbody>
          </table>"""
text = text.replace(old_llm, new_llm)


# Replace Str list
old_str = """          <ul className="list">
            {structuralProfiles.map((p) => (
              <li key={p.id}>
                <b>{p.name}</b> · {p.min_chunk_chars}-{p.max_chunk_chars}c · {p.patterns?.length ?? 0} Muster
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

new_str = """          <table className="grid">
            <thead>
              <tr><th>Name</th><th>Configuration</th><th>Actions</th></tr>
            </thead>
            <tbody>
              {structuralProfiles.map((p) => (
                <Fragment key={p.id}>
                  <tr>
                    <td>{p.name}</td>
                    <td>{p.min_chunk_chars}-{p.max_chunk_chars}c · {p.patterns?.length ?? 0} Muster</td>
                    <td className="col-actions">
                      {confirmDeleteStrId === p.id ? (
                        <span className="confirm">
                          Delete?
                          <button className="icon-btn danger" title="Confirm delete" onClick={() => void api.deleteStructuralProfile({ id: p.id }).then(reload)}>✓</button>
                          <button className="cancel-btn" onClick={() => setConfirmDeleteStrId(null)}>Cancel</button>
                        </span>
                      ) : (
                        <div className="actions-menu">
                          <button className="menu-trigger" title="Actions" onClick={() => setMenuForStr(menuForStr === p.id ? null : p.id)}>⋯</button>
                          {menuForStr === p.id && (
                            <>
                              <div className="menu-backdrop" onClick={() => setMenuForStr(null)} />
                              <div className="row-menu">
                                <button onClick={() => { setMenuForStr(null); startEditStr(p); }}>✏️ Edit profile</button>
                                <button onClick={() => { setMenuForStr(null); setConfirmDeleteStrId(p.id); }}>🗑️ Delete profile</button>
                              </div>
                            </>
                          )}
                        </div>
                      )}
                    </td>
                  </tr>
                </Fragment>
              ))}
              {structuralProfiles.length === 0 && <tr><td colSpan={3} className="muted" style={{textAlign:"center"}}>keine Profil-Vorlagen</td></tr>}
            </tbody>
          </table>"""
text = text.replace(old_str, new_str)

# Also add the menuFor state!
text = text.replace('  const [confirmDeleteStrId, setConfirmDeleteStrId] = useState<number | null>(null);', '  const [confirmDeleteStrId, setConfirmDeleteStrId] = useState<number | null>(null);\n  const [menuForLlm, setMenuForLlm] = useState<number | null>(null);\n  const [menuForStr, setMenuForStr] = useState<number | null>(null);')


with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)

print("done")
