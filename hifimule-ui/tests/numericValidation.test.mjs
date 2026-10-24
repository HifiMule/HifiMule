import assert from 'node:assert/strict';
import test from 'node:test';
import {numeric} from './numericTestHelpers.mjs';
const {numericError, monthListError, fieldFeedback, revealInvalid} = numeric;
test('raw numeric validation distinguishes blank, zero, fractions, finite values and exact integers', () => {
 const count = {required:true, integer:true, min:0, max:4294967295};
 for (const value of ['', ' ', '-1', '1.5', '1.00000000000000001', '4294967296', 'Infinity', 'NaN', '0x10', '1e9999']) assert.ok(numericError(value,count), value);
 for (const value of ['0','4294967295','1.0','1e2']) assert.equal(numericError(value,count),undefined,value);
 assert.equal(numericError('',{min:0}),undefined); assert.equal(numericError('0',{min:0}),undefined);
 for (const value of ['0.001','100000',' 256.125 ']) assert.equal(numericError(value,{min:.001,max:100000,decimals:3}),undefined,value);
 for (const value of ['0','0.0001','100000.001','1.2345']) assert.ok(numericError(value,{min:.001,max:100000,decimals:3}),value);
});
test('unit conversions reject overflow and rounding to zero, allow arbitrary positive fractions', () => {
 for (const scale of [3600,1073741824]) {
  assert.ok(numericError(String(Number.MAX_SAFE_INTEGER),{min:0,scale}));
  assert.ok(numericError('0.0000000000001',{min:0,scale}));
  assert.ok(numericError('1e-999',{min:0,scale}));
  assert.equal(numericError('0',{min:0,scale}),undefined);
  assert.equal(numericError('0.1234567',{min:0,scale}),undefined);
 }
 assert.ok(numericError('3.5e38',{min:0,float32:true}));
 assert.ok(numericError('1e-50',{min:0,float32:true}));
 assert.equal(numericError('0.1234567',{min:0,float32:true}),undefined);
});
test('Shoelace validity waits for the initial render and uses the latest correction',async()=>{
 const old=globalThis.document;globalThis.document={createElement:()=>({setAttribute(){},classList:{toggle(){}}})};
 try {
  let ready=false,complete;const control={tagName:'SL-INPUT',setAttribute(){},getAttribute(){},append(){},updateComplete:new Promise(resolve=>{complete=resolve;}),setCustomValidity(value){assert.ok(ready);this.error=value;},focus(){assert.ok(ready);this.focused=true;}};
  fieldFeedback(control,'Initial error');fieldFeedback(control,undefined,'Corrected');revealInvalid(control);assert.equal(control.error,undefined);assert.equal(control.focused,undefined);
  ready=true;complete();await control.updateComplete;await Promise.resolve();assert.equal(control.error,'');assert.equal(control.focused,true);
 } finally {globalThis.document=old;}
});
test('month lists reject every invalid token and preserve wrapping/empty windows', () => {
 for(const value of ['0','13','1,foo','1,,2','1,','1.5','Infinity']) assert.ok(monthListError(value),value);
 for(const value of ['', '12, 1', '3,3']) assert.equal(monthListError(value),undefined,value);
});
test('inline messages use validity, description, correction and disclosure focus', () => {
 const old = globalThis.document;
 globalThis.document = {createElement:()=>({setAttribute(){},classList:{toggle(){}}})};
 try {
  const attrs={}, details={open:false,matches:()=>true,parentElement:null}; let focused=0;
  const control={tagName:'SL-INPUT',parentElement:details,setAttribute:(k,v)=>attrs[k]=v,getAttribute:k=>attrs[k],append(){},setCustomValidity(value){this.error=value;},focus(){focused++;}};
  fieldFeedback(control,'Bad input','Hint'); assert.equal(control.error,'Bad input'); assert.equal(attrs['aria-invalid'],'true'); assert.equal(control._numericFeedback.textContent,'Bad input'); assert.ok(attrs['aria-describedby']);
  revealInvalid(control); assert.equal(details.open,true); assert.equal(focused,1);
  fieldFeedback(control,undefined,'Hint'); assert.equal(control.error,''); assert.equal(attrs['aria-invalid'],'false'); assert.equal(control._numericFeedback.textContent,'Hint');
 } finally {globalThis.document=old;}
});
