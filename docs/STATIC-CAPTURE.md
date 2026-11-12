# Native static capture

Use an ordinary request such as: “Capture page.html into preview.png at 800 by 600.” The HTML must already be in the workspace. The harness proposes its native capture operation; approve the displayed input and new output name. An explicitly configured agent must permit `capture`, just as it permits its other operations. The existing Rust core handles the operation through the ordinary command flow.

For manual use, Agent settings → Investigation preferences → Inspect a project file opens File investigation. Enter the HTML path, a new output filename and optional dimensions. Leave the font blank for a supported Android system face, or enter a workspace TrueType font path. Choose Capture local HTML and review. The result identifies the exact saved PNG and rendering scope. Project → Save a project file to default storage exports that PNG through the existing destination controls.

Defaults are 800 × 600 CSS pixels. The viewport is fixed and clips overflow. Input HTML is bounded to256KiB; capture dimensions to2048 per side and1Mi pixels; selected font input to16MiB. The engine uses original CSS/parser/cascade/layout/font/painting/PNG code. Backgrounds and supported text are actual pixels. The current supported CSS values include display none/block/inline, px dimensions/margins/padding/font-size/line-height, selected named and hexadecimal colors, inheritance and author importance/specificity/source order. Unsupported styles are counted, and unsupported layout behavior is reported in the operation result.

The capture path paints fetched HTML, its approved CSS/PNG resources and the completed DOM produced by the page-program option. Inspection records identify the source, resource scope and observed geometry. The PNG is saved in the workspace; use image attachments to send it to a model.

Relative resources now work: for `pages/index.html`, `<link rel="stylesheet" href="style.css">` reads `pages/style.css`, and `<img src="images/chart.png" width="300">` reads the relative PNG and preserves its aspect ratio. CSS width/height override HTML width/height hints, including `auto`. Inline `data:image/png;base64,...` is also decoded. The result lists each loaded resource and any unresolved resource.

PNG decoding preserves16-bit samples internally. The capture surface paints8-bit nearest-neighbour pixels. Normal and recursive capture share the same local loader and rendering core. Resource records identify the selected source and observed coverage.
