"""Provenance for reportlab-mixed-fields.pdf, generated independently with ReportLab 4.4.9.

Rust tests include the PDF bytes and do not require Python or ReportLab at runtime.
"""
from pathlib import Path
from reportlab.pdfgen import canvas
from reportlab.lib.colors import black, white

path = Path(__file__).with_suffix('.pdf')
c = canvas.Canvas(str(path), pagesize=(612, 792))
c.drawString(60, 730, 'Independent checkbox compatibility planning fixture')
c.acroForm.checkbox(name='Consent', tooltip='Consent', x=60, y=650, size=20,
    checked=False, buttonStyle='check', shape='square', borderStyle='solid',
    borderWidth=1, borderColor=black, fillColor=white, textColor=black, forceBorder=True)
c.acroForm.textfield(name='Name', x=60, y=580, width=250, height=24,
    value='Original', fontName='Helvetica', fontSize=12, borderWidth=1,
    borderStyle='solid', borderColor=black, fillColor=white, textColor=black,
    forceBorder=True, maxlen=40)
c.showPage()
c.save()
