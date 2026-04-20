const { shouldCollapse, cleanPastedText } = require('./src/input/paste_refs.ts');

const multiline = "line1\nline2\nline3";
const cleaned = cleanPastedText(multiline);
console.log('Original:', JSON.stringify(multiline));
console.log('Cleaned:', JSON.stringify(cleaned));
console.log('Should collapse:', shouldCollapse(cleaned));
console.log('Line count:', (cleaned.match(/\n/g) || []).length);
