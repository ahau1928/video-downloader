import vm from 'node:vm';
import fs from 'node:fs';
import assert from 'node:assert/strict';
const source=fs.readFileSync(new URL('../extension/popup.js',import.meta.url),'utf8');
async function run({url='https://www.youtube.com/watch?v=test',permission=true,nativeError=false}={}){
 const calls=[],elements={};
 const get=id=>elements[id]??=( {textContent:'',disabled:true,classList:{toggle(){}},addEventListener(name,cb){this[name]=cb}} );
 const chrome={tabs:{async query(){return [{id:7,url,title:'测试视频'}]}},permissions:{async request(value){calls.push(['permission',value]);return permission}},cookies:{async getAllCookieStores(){return [{id:'correct-profile',tabIds:[7]},{id:'other-profile',tabIds:[8]}]},async getAll(query){calls.push(['cookies',query]);return [{domain:'.youtube.com',path:'/',name:'test',value:'synthetic',secure:true,httpOnly:true,hostOnly:false}]}},runtime:{async sendNativeMessage(host,msg){calls.push(['native',host,msg]);if(nativeError)throw Error('Native host not found');return {ok:true,message:'发送成功'}}}};
 vm.runInNewContext(source,{chrome,document:{getElementById:get},URL});
 await new Promise(setImmediate);
 return {calls,get};
}
let t=await run();assert.equal(t.get('site').textContent,'YouTube');assert.equal(t.calls.length,0,'must not read cookies on popup open');await t.get('send').click();
assert.deepEqual(t.calls[0][1].origins[0],'https://*.youtube.com/*');assert.equal(t.calls[1][1].storeId,'correct-profile');assert.equal(t.calls[2][2].action,'send');
t=await run({permission:false});await t.get('authorize').click();assert.equal(t.calls.length,1,'permission refusal must stop access');
t=await run({url:'https://youtube.com.evil.test/video'});assert.equal(t.get('send').disabled,true);assert.equal(t.calls.length,0);
t=await run({nativeError:true});await t.get('send').click();assert.match(t.get('status').textContent,/启用浏览器连接/);
console.log('Extension tests passed: explicit permission, site scope, active profile, denial, unsupported URLs, connection guidance');
