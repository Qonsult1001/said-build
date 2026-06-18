const U={ms:1,msec:1,msecs:1,s:1e3,sec:1e3,secs:1e3,second:1e3,seconds:1e3,m:6e4,min:6e4,mins:6e4,minute:6e4,minutes:6e4,h:36e5,hr:36e5,hrs:36e5,hour:36e5,hours:36e5,d:864e5,day:864e5,days:864e5,w:6048e5,week:6048e5,weeks:6048e5,mo:2629800000,month:2629800000,months:2629800000,y:31557600000,yr:31557600000,year:31557600000,years:31557600000};
function ms(str){const m=/^(-?(?:\d+)?\.?\d+) *([a-z]+)?$/i.exec(String(str).trim());if(!m)return NaN;const n=parseFloat(m[1]);const u=(m[2]||'ms').toLowerCase();const f=U[u];return f===undefined?NaN:n*f;}
module.exports={ms};
