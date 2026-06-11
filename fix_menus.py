import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

# Replace the trigger buttons
text = text.replace('onClick={() => setMenuForLlm(menuForLlm === p.id ? null : p.id)}>⋮</button>', 'onClick={() => setMenuForLlm(menuForLlm === p.id ? null : p.id)}>⋯</button>')
text = text.replace('onClick={() => setMenuForStr(menuForStr === p.id ? null : p.id)}>⋮</button>', 'onClick={() => setMenuForStr(menuForStr === p.id ? null : p.id)}>⋯</button>')
text = text.replace('onClick={() => setMenuForEndpoint(menuForEndpoint === `llm-${ep.id}` ? null : `llm-${ep.id}`)}>⋮</button>', 'onClick={() => setMenuForEndpoint(menuForEndpoint === `llm-${ep.id}` ? null : `llm-${ep.id}`)}>⋯</button>')
text = text.replace('onClick={() => setMenuForEndpoint(menuForEndpoint === `emb-${m.id}` ? null : `emb-${m.id}`)}>⋮</button>', 'onClick={() => setMenuForEndpoint(menuForEndpoint === `emb-${m.id}` ? null : `emb-${m.id}`)}>⋯</button>')

# Replace the dropdown divs
text = text.replace('{menuForLlm === p.id && (\n                            <div className="menu-dropdown">', '{menuForLlm === p.id && (\n                            <>\n                              <div className="menu-backdrop" onClick={() => setMenuForLlm(null)} />\n                              <div className="row-menu">')
text = text.replace('{menuForStr === p.id && (\n                            <div className="menu-dropdown">', '{menuForStr === p.id && (\n                            <>\n                              <div className="menu-backdrop" onClick={() => setMenuForStr(null)} />\n                              <div className="row-menu">')
text = text.replace('{menuForEndpoint === `llm-${ep.id}` && (\n                        <div className="menu-dropdown">', '{menuForEndpoint === `llm-${ep.id}` && (\n                        <>\n                          <div className="menu-backdrop" onClick={() => setMenuForEndpoint(null)} />\n                          <div className="row-menu">')
text = text.replace('{menuForEndpoint === `emb-${m.id}` && (\n                        <div className="menu-dropdown">', '{menuForEndpoint === `emb-${m.id}` && (\n                        <>\n                          <div className="menu-backdrop" onClick={() => setMenuForEndpoint(null)} />\n                          <div className="row-menu">')

# Replace the closing tags
# For menuForLlm and menuForStr
text = text.replace('                            </div>\n                          )}', '                            </div>\n                            </>\n                          )}')
# For menuForEndpoint
text = text.replace('                        </div>\n                      )}', '                        </div>\n                        </>\n                      )}')


with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)

print("done")
