const HOST='com.jasonpan.video_downloader';
const $=id=>document.getElementById(id);
let current;
function show(message,error=false){$('status').textContent=message;$('status').classList.toggle('error',error)}
function supported(url){try{const u=new URL(url);if(u.protocol!=='https:'||u.username||u.password||u.port)return null;const h=u.hostname;if(h==='youtu.be'||h==='youtube.com'||h.endsWith('.youtube.com'))return 'youtube';if(h==='vimeo.com'||h.endsWith('.vimeo.com'))return 'vimeo';}catch{}return null}
async function initialize(){const [tab]=await chrome.tabs.query({active:true,currentWindow:true});const site=supported(tab?.url);if(!site){$('site').textContent='请打开视频页面';$('title').textContent='支持 YouTube 和 Vimeo';return}current={tab,site};$('site').textContent=site==='youtube'?'YouTube':'Vimeo';$('title').textContent=tab.title||tab.url;$('send').disabled=false;$('authorize').disabled=false;}
async function native(message){const response=await chrome.runtime.sendNativeMessage(HOST,message);if(!response?.ok)throw Error(response?.message||'下载器未返回有效响应');return response;}
function errorText(e){const text=String(e?.message||e);if(/host|native|specified|receiving end/i.test(text))return '无法连接下载器。请先安装 0.3.0 或更新版，在“网络与账号”点击“启用浏览器连接”，再检测一次。';return text}
async function send(action){if(!current)return;const {site,tab}=current;const origins=[`https://*.${site}.com/*`];
 try{
  // Request only the current site's permission, directly from this user gesture.
  if(!await chrome.permissions.request({origins})){show('未授权，未读取或发送登录信息。');return}
  $('send').disabled=true;$('authorize').disabled=true;show('正在连接本机下载器…');
  const stores=await chrome.cookies.getAllCookieStores();const store=stores.find(s=>s.tabIds.includes(tab.id));if(!store)throw Error('无法确定当前浏览器个人资料，请使用普通窗口重试');
  const cookies=await chrome.cookies.getAll({domain:`${site}.com`,storeId:store.id});
  if(!cookies.length)throw Error('没有找到当前网站的 Cookie，请先登录后再试');
  const result=await native({action,url:tab.url,cookies:cookies.map(({domain,path,name,value,secure,httpOnly,hostOnly,expirationDate})=>({domain,path,name,value,secure,httpOnly,hostOnly,expirationDate}))});show(result.message);
 }catch(e){show(errorText(e),true)}finally{$('send').disabled=false;$('authorize').disabled=false}
}
$('send').addEventListener('click',()=>send('send'));
$('authorize').addEventListener('click',()=>send('authorize'));
$('ping').addEventListener('click',async()=>{try{show((await native({action:'ping'})).message)}catch(e){show(errorText(e),true)}});
initialize().catch(e=>show(errorText(e),true));
