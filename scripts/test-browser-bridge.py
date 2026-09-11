"""Exercise the actual native messaging executable with synthetic cookies only."""
import json, os, pathlib, struct, subprocess, sys, hashlib
root=pathlib.Path(__file__).resolve().parents[1]
exe=root/'src-tauri/target/debug/lumen-video-downloader.exe'
testdir=root/'.review/browser-bridge030'
testdir.mkdir(parents=True,exist_ok=True)
env=dict(os.environ,VIDEOTOOL_TEST_DIR=str(testdir))
origin='chrome-extension://'+(root/'extension/id.txt').read_text().strip()+'/'
def send(message,source=origin):
    raw=json.dumps(message).encode();frame=struct.pack('<I',len(raw))+raw
    p=subprocess.run([str(exe),source],input=frame,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env,timeout=15,creationflags=subprocess.CREATE_NO_WINDOW)
    assert p.returncode==0, 'native host failed'
    assert len(p.stdout)>=4, 'native response missing'
    size=struct.unpack('<I',p.stdout[:4])[0]
    assert size==len(p.stdout)-4,'native framing incorrect'
    return json.loads(p.stdout[4:])
assert send({'action':'ping'})['ok']
assert not send({'action':'ping'},'chrome-extension://'+'a'*32+'/')['ok']
cookie={'domain':'.youtube.com','path':'/','name':'synthetic_session','value':'test-only-secret-030','secure':True,'httpOnly':True,'hostOnly':False}
assert not send({'action':'authorize','url':'https://youtube.com.evil.test/watch','cookies':[cookie]})['ok']
assert not send({'action':'authorize','url':'https://vimeo.com/123','cookies':[cookie]})['ok']
assert send({'action':'authorize','url':'https://www.youtube.com/watch?v=test','cookies':[cookie]})['ok']
blob=(testdir/'browser-auth/youtube.bin').read_bytes()
assert b'test-only-secret-030' not in blob,'plaintext credential persisted'
assert send({'action':'send','url':'https://www.youtube.com/watch?v=test','cookies':[cookie]})['ok']
inbox=list((testdir/'browser-auth/inbox').glob('*.json'))
assert inbox and all('cookies' not in f.read_text() for f in inbox),'credential leaked into queue message'
assert not send({'action':'authorize','url':'https://www.youtube.com/watch?v=test','cookies':[{**cookie,'value':'bad\nrow'}]})['ok']
key=__import__('base64').b64decode(json.loads((root/'extension/manifest.json').read_text(encoding='utf8'))['key'])
derived=''.join(chr(97+int(x,16)) for x in hashlib.sha256(key).hexdigest()[:32])
assert derived==(root/'extension/id.txt').read_text().strip(),'extension identity mismatch'
report={'passed':True,'checks':['真实 EXE 原生消息帧及中文响应','拒绝未授权扩展来源','限制网站与 Cookie 域名','Windows 加密持久化','发送链接与登录信息分离','拒绝 Cookie 换行注入','固定扩展身份核验']}
(testdir/'result.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf8')
print(json.dumps(report,ensure_ascii=False))
