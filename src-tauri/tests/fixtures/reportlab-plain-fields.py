"""Independent AcroForm fixture generator; tests use the committed PDF, not Python.

Generated with ReportLab 4.4.9. Two flat single-line Helvetica widgets retain
distinct fixed font sizes, solid borders, background and text colors.
"""
from pathlib import Path
from reportlab.pdfgen import canvas
from reportlab.lib.colors import black, white, Color

output = Path(__file__).with_suffix('.pdf')
c = canvas.Canvas(str(output), pagesize=(612, 792), pageCompression=1)
c.setFont('Helvetica', 16)
c.drawString(60, 730, 'Independent AcroForm compatibility fixture')
c.setFont('Helvetica', 11)
c.drawString(60, 690, 'Name')
c.acroForm.textfield(name='Name', tooltip='Name', x=60, y=650, width=250, height=24,
    value='Original', fontName='Helvetica', fontSize=12, textColor=black,
    borderWidth=1, borderStyle='solid', borderColor=black, fillColor=white,
    forceBorder=True, maxlen=40)
c.drawString(60, 610, 'City')
c.acroForm.textfield(name='City', tooltip='City', x=60, y=570, width=250, height=26,
    value='', fontName='Helvetica', fontSize=10, textColor=Color(0.1, 0.2, 0.3),
    borderWidth=1, borderStyle='solid', borderColor=Color(0.2, 0.3, 0.4), fillColor=Color(0.9, 0.95, 1),
    forceBorder=True, maxlen=60)
c.showPage()
c.save()
