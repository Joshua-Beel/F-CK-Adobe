"""Provenance: reportlab-radio-fields.pdf was generated with ReportLab 4.4.9.

Rust tests include its bytes; Python and ReportLab are not runtime dependencies.
"""
from pathlib import Path
from reportlab.pdfgen import canvas
from reportlab.lib.colors import black, white

c = canvas.Canvas(str(Path(__file__).with_suffix('.pdf')), pagesize=(612, 792))
c.drawString(60, 730, 'Independent standard radio-group planning fixture')
for value, y in [('Choice A', 650), ('Choice B', 610), ('Choice C', 570)]:
    c.acroForm.radio(name='Preference', value=value, selected=value == 'Choice B',
        x=60, y=y, size=20, buttonStyle='circle', shape='circle',
        borderStyle='solid', borderWidth=1, borderColor=black, fillColor=white,
        textColor=black, fieldFlags='radio required noToggleToOff')
    c.drawString(95, y+5, value)
c.showPage()
c.save()
