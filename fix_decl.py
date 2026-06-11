import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

# Find the start of the return statement
idx = text.find('  return (\n    <div className="page">\n      <h2>Settings</h2>')

if idx != -1:
    # Find the endpointForm
    start_form = text.find('  const endpointForm = (\n    <>\n', idx)
    end_form = text.find('    </>\n  );\n', start_form) + len('    </>\n  );\n')
    
    if start_form != -1 and end_form != -1:
        form_str = text[start_form:end_form]
        
        # Remove from inside return
        text = text[:start_form] + text[end_form:]
        
        # Insert before return
        text = text[:idx] + form_str + '\n' + text[idx:]
        
        with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
            f.write(text)
        print("Moved endpointForm outside return block")

