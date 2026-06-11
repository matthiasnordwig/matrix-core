import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

# We need to extract lines 1313 to 1421 which is the `<div className="form">...</div>`
match = re.search(r'(<div className="form">.*?</>\n\s*\)\}\n\s*</div>)', text, re.DOTALL)
if match:
    form_content = match.group(1)
    
    # We define `const endpointForm = (<>{form_content}</>);`
    endpoint_form_decl = "  const endpointForm = (\n    <>\n" + "\n".join(["      " + l for l in form_content.split("\n")]) + "\n    </>\n  );\n"
    
    # Insert it before `return (`
    text = text.replace('  return (\n    <div className="tab-content">', endpoint_form_decl + '\n  return (\n    <div className="tab-content">')
    
    # Remove the original `<div className="card">...</div>` that contained the form
    old_card = r'<div className="card">\n\s*<h3>\{editing \? "Endpunkt bearbeiten" : "Endpunkt hinzufügen"\}</h3>\n\s*<p className="muted">.*?</p>\n\s*<div className="form">.*?</>\n\s*\)\}\n\s*</div>\n\s*</div>'
    text = re.sub(old_card, '', text, flags=re.DOTALL)

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)

print("done endpointForm")
