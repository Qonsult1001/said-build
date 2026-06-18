const RE=/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
function uuidValidate(s){ return typeof s==='string' && RE.test(s); }
function uuidVersion(s){ if(!uuidValidate(s)) throw new Error('invalid uuid'); return parseInt(s.charAt(14),16); }
function isV4(s){ return uuidValidate(s) && uuidVersion(s)===4; }
module.exports={uuidValidate,uuidVersion,isV4};
