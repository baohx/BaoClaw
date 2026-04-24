/** BaoClaw Web Frontend — with tab support */
const $=id=>document.getElementById(id);
const messagesEl=$('messages'),inputEl=$('input'),btnSend=$('btn-send'),btnAbort=$('btn-abort');
const sessionInfoEl=$('session-info'),statusTextEl=$('status-text'),projectListEl=$('project-list');
const searchInput=$('search-input'),searchOverlay=$('search-overlay'),searchResults=$('search-results');
const tabBarEl=$('tab-bar');

marked.setOptions({highlight:(code,lang)=>{
  if(lang&&hljs.getLanguage(lang))return hljs.highlight(code,{language:lang}).value;
  return hljs.highlightAuto(code).value;
},breaks:true});

function scrollToBottom(){const el=getActiveMsgEl();el.scrollTop=el.scrollHeight;}
function setStatus(t,c){statusTextEl.textContent=t;statusTextEl.className=c||'';}
function fmtTok(n){return n>=1000?(n/1000).toFixed(1)+'k':String(n);}
function esc(s){return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');}
function showImageModal(src){const m=document.createElement('div');m.className='image-modal';m.innerHTML='<img src="'+src+'">';m.onclick=()=>m.remove();document.body.appendChild(m);}

// ═══════════════════════════════════════════════════════════════
// Tab state management
// ═══════════════════════════════════════════════════════════════
const tabs=new Map(); // cwd -> {ws, msgEl, state, label}
let activeTab=null; // cwd of active tab

function createTab(cwd, label) {
  const msgEl=document.createElement('div');
  msgEl.className='tab-messages';
  msgEl.style.display='none';
  messagesEl.parentNode.insertBefore(msgEl, messagesEl);
  const state={currentText:'',isStreaming:false,toolCount:0,queryStartTime:0,
    currentAssistantEl:null,pendingTools:new Map(),sessionId:'',msgCount:0,contextTokens:0,totalCost:0,loopNum:0,loopToolCount:0,lastStreamType:''};
  const tab={ws:null,msgEl,state,label,cwd};
  tabs.set(cwd,tab);
  renderTabBar();
  return tab;
}

function updateSessionInfo(tab,cwd){
  sessionInfoEl.innerHTML='Session: <span>'+(tab.state.sessionId||'?')+'</span><br>CWD: <span>'+esc(cwd)+'</span><br>Messages: <span>'+(tab.state.msgCount||0)+'</span><br>Context: <span>'+fmtTok(tab.state.contextTokens)+'</span>'+(tab.state.totalCost>0?' · <span style="color:var(--yellow)">$'+tab.state.totalCost.toFixed(4)+'</span>':'');
}
function activateTab(cwd) {
  const tab=tabs.get(cwd);
  if(!tab)return;
  // Hide all tab message containers, show the active one
  tabs.forEach(t=>{t.msgEl.style.display='none';});
  tab.msgEl.style.display='block';
  activeTab=cwd;
  // Update session info
  updateSessionInfo(tab,cwd);
  document.title='BaoClaw - '+(cwd.split('/').pop()||cwd);
  setStatus(tab.ws?.readyState===1?'Connected':'Connecting…',tab.ws?.readyState===1?'connected':'');
  renderTabBar();
  // Update project list active state
  projectListEl.querySelectorAll('.project-item').forEach(el=>{
    el.classList.toggle('active',el.dataset.cwd===cwd);
  });
  inputEl.focus();
}

function closeTab(cwd) {
  if(tabs.size<=1)return; // keep at least one tab
  const tab=tabs.get(cwd);
  if(!tab)return;
  if(tab.ws)try{tab.ws.close();}catch{}
  tab.msgEl.remove();
  tabs.delete(cwd);
  if(activeTab===cwd){
    const remaining=[...tabs.keys()];
    if(remaining.length)activateTab(remaining[remaining.length-1]);
  }
  renderTabBar();
}

function renderTabBar() {
  tabBarEl.innerHTML='';
  tabs.forEach((tab,cwd)=>{
    const el=document.createElement('div');
    el.className='tab'+(cwd===activeTab?' active':'');
    const name=tab.label||cwd.split('/').pop()||cwd;
    el.innerHTML=esc(name)+'<span class="tab-close" title="Close">\u2715</span>';
    el.onclick=(e)=>{
      if(e.target.classList.contains('tab-close')){closeTab(cwd);return;}
      activateTab(cwd);
    };
    tabBarEl.appendChild(el);
  });
}

// Get the active tab's message container
function getActiveMsgEl(){return tabs.get(activeTab)?.msgEl||messagesEl;}
function getActiveState(){return tabs.get(activeTab)?.state||{};}
function getActiveWs(){return tabs.get(activeTab)?.ws||null;}

// ═══════════════════════════════════════════════════════════════
// Message rendering (uses active tab's container)
// ═══════════════════════════════════════════════════════════════
function addUserMessage(text){
  const el=document.createElement('div');el.className='msg user';el.textContent=text;
  getActiveMsgEl().appendChild(el);scrollToBottom();
}
function ensureAssistantMessage(){
  const s=getActiveState();
  if(!s.currentAssistantEl){
    const el=document.createElement('div');el.className='msg assistant';
    el.innerHTML='<div class="msg-header">BaoClaw</div><div class="msg-content"></div>';
    getActiveMsgEl().appendChild(el);s.currentAssistantEl=el;
    s._currentTextEl=null; // current text segment being streamed into
  }
  return s.currentAssistantEl.querySelector('.msg-content');
}
function ensureTextSegment(){
  const s=getActiveState();
  const container=ensureAssistantMessage();
  if(!s._currentTextEl){
    s._currentTextEl=document.createElement('div');
    s._currentTextEl.className='msg-body';
    container.appendChild(s._currentTextEl);
  }
  return s._currentTextEl;
}

function renderAssistantText(){
  const body=ensureTextSegment(),s=getActiveState();
  body.innerHTML=marked.parse(s.currentText);
  body.querySelectorAll('pre').forEach(pre=>{
    if(pre.querySelector('.copy-btn'))return;pre.style.position='relative';
    const c=pre.querySelector('code'),lm=c?.className?.match(/language-(\w+)/);
    if(lm){const b=document.createElement('span');b.className='lang-badge';b.textContent=lm[1];pre.appendChild(b);}
    const btn=document.createElement('button');btn.className='copy-btn';btn.textContent='Copy';
    btn.onclick=()=>{navigator.clipboard.writeText(c?.textContent||pre.textContent);btn.textContent='Copied!';setTimeout(()=>btn.textContent='Copy',1500);};
    pre.appendChild(btn);
  });
  body.querySelectorAll('img').forEach(img=>{img.classList.add('tool-image');img.onclick=()=>showImageModal(img.src);});
  scrollToBottom();
}

function addToolCall(toolName,input,toolUseId){
  const s2=getActiveState();
  // Flush current text segment before inserting tool
  if(s2._currentTextEl){s2._currentTextEl=null;}
  const body=ensureAssistantMessage(),d=document.createElement('details');d.className='tool-call';
  let s=toolName;const inp=typeof input==='object'&&input?input:{};
  if(toolName==='Bash'&&inp.command)s='$ '+inp.command;
  else if(['FileRead','Read'].includes(toolName)&&inp.file_path)s='\u{1F4C4} '+inp.file_path;
  else if(['FileWrite','Write'].includes(toolName)&&inp.file_path)s='\u270F\uFE0F '+inp.file_path;
  else if(['FileEdit','Edit'].includes(toolName)&&inp.file_path)s='\u270E '+inp.file_path;
  else if(toolName==='GrepTool'&&inp.pattern)s='\u{1F50D} /'+inp.pattern+'/';
  else if(toolName==='GlobTool'&&inp.pattern)s='\u{1F4C2} '+inp.pattern;
  else if(toolName==='WebSearchTool'&&inp.query)s='\u{1F50E} "'+inp.query+'"';
  else if(toolName==='WebFetchTool'&&inp.url)s='\u{1F310} '+inp.url;
  else if(toolName==='AgentTool'&&inp.prompt)s='\u{1F916} '+inp.prompt;
  d.innerHTML='<summary>\u26A1 '+esc(s)+'</summary><div class="tool-body" id="tool-'+toolUseId+'">Running\u2026</div>';
  body.appendChild(d);getActiveState().pendingTools.set(toolUseId,{name:toolName,input:inp,el:d});scrollToBottom();
}

function addToolResult(toolUseId,output,isError){
  const st=getActiveState(),tool=st.pendingTools.get(toolUseId);st.pendingTools.delete(toolUseId);
  const el=tool?.el?.querySelector('.tool-body');if(!el)return;
  const name=tool?.name||'',inp=tool?.input||{},cls=isError?'tool-result-err':'tool-result-ok',pfx=isError?'\u2717 ':'\u2713 ';
  // WebSearch links
  if(['WebSearchTool','Search'].includes(name)&&!isError&&typeof output==='object'&&output){
    const r=output.results||[];
    if(r.length){el.className='tool-body tool-result-ok';
      el.innerHTML=r.slice(0,8).map(x=>'<div style="margin-bottom:6px"><a href="'+esc(x.url)+'" target="_blank" style="color:var(--blue)">'+esc(x.title)+'</a><br><span style="font-size:11px;color:var(--text-dim)">'+esc((x.snippet||'').slice(0,120))+'</span></div>').join('');
      scrollToBottom();return;}
  }
  // Images
  if(typeof output==='object'&&output&&Array.isArray(output.content)){
    const imgs=output.content.filter(c=>c?.type==='image'&&c?.data);
    if(imgs.length){el.className='tool-body';el.innerHTML='';
      for(const i of imgs){const ie=document.createElement('img');ie.src='data:'+(i.mimeType||'image/png')+';base64,'+i.data;ie.className='tool-image';ie.onclick=()=>showImageModal(ie.src);el.appendChild(ie);}
      scrollToBottom();return;}
  }
  // Generic text
  let text='';
  if(typeof output==='string')text=output;
  else if(typeof output==='object'&&output){
    if(name==='Bash')text=output.output||output.stdout||'';
    else if(['FileRead','Read'].includes(name))text=(output.lines_read||output.total_lines||'?')+' lines from '+(output.file_path||'');
    else if(['FileWrite','Write'].includes(name))text=(output.file_path||'')+' ('+(output.bytes_written||'?')+' bytes)';
    else if(name==='GrepTool')text=(output.matches||[]).length+' matches';
    else if(name==='GlobTool')text=(output.files||[]).length+' files';
    else if(name==='AgentTool')text=output.result||'done';
    else text=output.output||output.stdout||output.content||output.result||JSON.stringify(output).slice(0,500);
  }
  el.className='tool-body '+cls;el.textContent=pfx+text;scrollToBottom();
}

function addStatsBar(result){
  const s=getActiveState();if(!s.currentAssistantEl)return;
  const bar=document.createElement('div');bar.className='stats-bar';
  if(s.toolCount>0)bar.innerHTML+='<span class="stat stat-tools">\u26A1 '+s.toolCount+' tool'+(s.toolCount>1?'s':'')+'</span>';
  if(result.usage&&(result.usage.input_tokens>0||result.usage.output_tokens>0))
    bar.innerHTML+='<span class="stat stat-tokens">\u2191'+fmtTok(result.usage.input_tokens)+' \u2193'+fmtTok(result.usage.output_tokens)+'</span>';
  if(result.total_cost_usd>0)bar.innerHTML+='<span class="stat stat-cost">$'+result.total_cost_usd.toFixed(4)+'</span>';
  if(s.queryStartTime>0)bar.innerHTML+='<span class="stat">'+((Date.now()-s.queryStartTime)/1000).toFixed(1)+'s</span>';
  s.currentAssistantEl.appendChild(bar);scrollToBottom();
}

function addPermissionRequest(toolName,input,toolUseId){
  const s3=getActiveState();if(s3._currentTextEl)s3._currentTextEl=null;
  const body=ensureAssistantMessage(),div=document.createElement('div');div.className='permission-dialog';
  const ps=Object.entries(input||{}).slice(0,3).map(([k,v])=>k+'='+String(v).slice(0,40)).join(', ');
  div.innerHTML='<div class="perm-title">\u26A0 Permission: '+esc(toolName)+'</div><div style="color:var(--text-dim);margin-bottom:8px;font-size:12px">'+esc(ps)+'</div><button class="allow" data-d="allow">Allow</button> <button class="allow" data-d="allow_always">Always</button> <button class="deny" data-d="deny">Deny</button>';
  div.querySelectorAll('button').forEach(b=>{b.onclick=()=>{const w=getActiveWs();if(w)w.send(JSON.stringify({action:'permission',tool_use_id:toolUseId,decision:b.dataset.d,rule:b.dataset.d==='allow_always'?toolName:undefined}));div.innerHTML='<div style="color:var(--text-dim)">\u26A0 '+esc(toolName)+': '+b.dataset.d+'</div>';};});
  body.appendChild(div);scrollToBottom();
}

function addSystemMessage(html){
  const el=document.createElement('div');el.className='msg assistant';
  el.innerHTML='<div class="msg-header">System</div><div class="msg-body">'+html+'</div>';
  getActiveMsgEl().appendChild(el);scrollToBottom();
}

// ═══════════════════════════════════════════════════════════════
// Projects & WebSocket per tab
// ═══════════════════════════════════════════════════════════════
function loadProjects(){const w=getActiveWs();if(w?.readyState===1)w.send(JSON.stringify({action:'rpc',method:'projectsList'}));}

function renderProjects(projects){
  projectListEl.innerHTML='';
  for(const p of projects){
    const div=document.createElement('div');div.className='project-item'+(p.cwd===activeTab?' active':'');
    div.dataset.cwd=p.cwd;
    const sp=p.cwd.length>28?'\u2026'+p.cwd.slice(-27):p.cwd;
    div.innerHTML='<div class="proj-name">'+esc(p.description)+'</div><div class="proj-path">'+esc(sp)+'</div>';
    div.onclick=()=>openProject(p);
    projectListEl.appendChild(div);
  }
}

function openProject(p){
  // If tab already exists, just activate it
  if(tabs.has(p.cwd)){activateTab(p.cwd);return;}
  // Create new tab and connect
  const tab=createTab(p.cwd, p.description);
  activateTab(p.cwd);
  connectTab(p.cwd);
}

function connectTab(cwd){
  const tab=tabs.get(cwd);if(!tab)return;
  const wsUrl=(location.protocol==='https:'?'wss:':'ws:')+'//'+location.host+'/?cwd='+encodeURIComponent(cwd);
  const w=new WebSocket(wsUrl);
  tab.ws=w;
  w.onopen=()=>{if(activeTab===cwd)setStatus('Connected','connected');};
  w.onmessage=(evt)=>handleTabMessage(tab,JSON.parse(evt.data));
  w.onclose=()=>{if(activeTab===cwd)setStatus('Disconnected','error');};
  w.onerror=()=>{if(activeTab===cwd)setStatus('Error','error');};
}

// ═══════════════════════════════════════════════════════════════
// Search
// ═══════════════════════════════════════════════════════════════
let searchDebounce=null;
searchInput.addEventListener('input',()=>{
  clearTimeout(searchDebounce);const q=searchInput.value.trim();
  if(!q){searchOverlay.classList.add('hidden');return;}
  searchDebounce=setTimeout(()=>{
    const hits=[],container=getActiveMsgEl();
    container.querySelectorAll('.msg').forEach(el=>{
      const t=el.textContent||'',idx=t.toLowerCase().indexOf(q.toLowerCase());
      if(idx>=0)hits.push({el,role:el.classList.contains('user')?'You':'BaoClaw',snippet:t.slice(Math.max(0,idx-30),idx+q.length+50),query:q});
    });
    searchResults.innerHTML=hits.length?hits.map((h,i)=>'<div class="search-hit" data-idx="'+i+'"><div class="hit-role">'+h.role+'</div><div class="hit-text">'+h.snippet.replace(new RegExp(esc(h.query),'gi'),m=>'<mark>'+m+'</mark>')+'</div></div>').join(''):'<div style="padding:20px;color:var(--text-dim)">No results</div>';
    searchResults.querySelectorAll('.search-hit').forEach((el,i)=>{el.onclick=()=>{hits[i].el.scrollIntoView({behavior:'smooth',block:'center'});hits[i].el.style.outline='2px solid var(--accent)';setTimeout(()=>hits[i].el.style.outline='',2000);searchOverlay.classList.add('hidden');searchInput.value='';};});
    searchOverlay.classList.remove('hidden');
  },300);
});
$('search-close').onclick=()=>{searchOverlay.classList.add('hidden');searchInput.value='';};

// ═══════════════════════════════════════════════════════════════
// UI controls
// ═══════════════════════════════════════════════════════════════
function setBusy(b){btnSend.classList.toggle('hidden',b);btnAbort.classList.toggle('hidden',!b);inputEl.disabled=b;if(!b)inputEl.focus();}

function sendMessage(){
  const t=inputEl.value.trim(),w=getActiveWs();if(!t||!w||w.readyState!==1)return;
  // Handle slash commands
  if(t.startsWith('/')){
    inputEl.value='';inputEl.style.height='auto';
    if(t==='/compact'){w.send(JSON.stringify({action:'compact'}));addSystemMessage('\u{1F5DC}\uFE0F Compacting...');return;}
    if(t==='/history'){w.send(JSON.stringify({action:'rpc',method:'talkTail',params:{count:100}}));return;}
    if(t==='/clear'){getActiveMsgEl().innerHTML='';return;}
    if(t==='/abort'){doAbort();return;}
    // Unknown command — send as regular message
  }
  addUserMessage(t);inputEl.value='';inputEl.style.height='auto';
  const s=getActiveState();s.currentText='';s.isStreaming=false;s.toolCount=0;s.currentAssistantEl=null;s.queryStartTime=Date.now();
  setBusy(true);
  w.send(JSON.stringify({action:'submit',prompt:t}));
}

inputEl.addEventListener('keydown',e=>{if(e.key==='Enter'&&!e.shiftKey){e.preventDefault();sendMessage();}});
inputEl.addEventListener('input',()=>{inputEl.style.height='auto';inputEl.style.height=Math.min(inputEl.scrollHeight,150)+'px';});
btnSend.onclick=sendMessage;

function doAbort(){
  const w=getActiveWs();if(w?.readyState===1)w.send(JSON.stringify({action:'abort'}));
  addSystemMessage('\u26A0 Aborted');
  const s=getActiveState();s.currentText='';s.isStreaming=false;s.toolCount=0;s.currentAssistantEl=null;s._currentTextEl=null;s.queryStartTime=0;s.loopNum=0;s.loopToolCount=0;s.lastStreamType='';
  setBusy(false);
}
btnAbort.addEventListener('mousedown',e=>{e.preventDefault();e.stopPropagation();doAbort();});

$('btn-compact').onclick=()=>{const w=getActiveWs();if(w?.readyState===1)w.send(JSON.stringify({action:'compact'}));};
$('btn-history').onclick=()=>{const w=getActiveWs();if(w?.readyState===1)w.send(JSON.stringify({action:'rpc',method:'talkTail',params:{count:100}}));};
$('btn-clear').onclick=()=>{getActiveMsgEl().innerHTML='';};

$('btn-download-md').onclick=()=>{
  let md='# BaoClaw Conversation\n\n';
  getActiveMsgEl().querySelectorAll('.msg').forEach(el=>{
    if(el.classList.contains('user'))md+='## You\n\n'+el.textContent+'\n\n';
    else if(el.classList.contains('assistant')){const b=el.querySelector('.msg-body');md+='## BaoClaw\n\n'+(b?.textContent||'')+'\n\n';}
  });
  const a=document.createElement('a');a.href=URL.createObjectURL(new Blob([md],{type:'text/markdown'}));a.download='baoclaw-'+new Date().toISOString().slice(0,10)+'.md';a.click();
};

$('btn-download-pdf').onclick=()=>{
  const c=getActiveMsgEl().cloneNode(true);c.style.cssText='background:#1a1a2e;color:#e0e0e0;padding:20px;font-family:monospace';
  html2pdf().set({margin:10,filename:'baoclaw-'+new Date().toISOString().slice(0,10)+'.pdf',html2canvas:{scale:2,backgroundColor:'#1a1a2e'},jsPDF:{unit:'mm',format:'a4',orientation:'portrait'}}).from(c).save();
};

// ═══════════════════════════════════════════════════════════════
// Startup — create initial tab from server's cwd
// ═══════════════════════════════════════════════════════════════
(function init(){
  const initialCwd='__default__';
  const tab=createTab(initialCwd,'Connecting...');
  activateTab(initialCwd);
  const wsUrl=(location.protocol==='https:'?'wss:':'ws:')+'//'+location.host+'/';
  const w=new WebSocket(wsUrl);
  tab.ws=w;
  let inited=false;
  w.onopen=()=>setStatus('Connected','connected');
  w.onmessage=(evt)=>{
    const msg=JSON.parse(evt.data);
    if(!inited&&msg.type==='init'){
      inited=true;
      const realCwd=msg.cwd||initialCwd;
      if(realCwd!==initialCwd){tabs.delete(initialCwd);tab.cwd=realCwd;tabs.set(realCwd,tab);activeTab=realCwd;}
      tab.label=realCwd.split('/').pop()||realCwd;
      renderTabBar();
    }
    handleTabMessage(tab,msg);
  };
  w.onclose=()=>setStatus('Disconnected','error');
  w.onerror=()=>setStatus('Error','error');
})();

// Shared message handler for all tabs (used by connectTab and init)
function handleTabMessage(tab,msg){
  const cwd=tab.cwd,s=tab.state;
  const isActive=()=>activeTab===cwd;
  switch(msg.type){
    case 'init':{s.sessionId=msg.data.session_id||'';s.msgCount=msg.data.message_count||0;
      if(isActive()){updateSessionInfo(tab,cwd);setStatus('Connected','connected');}
      loadProjects();if(s.msgCount>0&&tab.ws?.readyState===1)tab.ws.send(JSON.stringify({action:'rpc',method:'talkTail',params:{count:100}}));break;}
    case 'stream':{const e=msg.data;if(!e?.type)break;
      switch(e.type){
        case 'assistant_chunk':s.currentText+=e.content||'';s.isStreaming=true;s.lastStreamType='chunk';if(isActive())renderAssistantText();break;
        case 'thinking_chunk':s.currentText+=e.content||'';s.isStreaming=true;if(isActive())renderAssistantText();break;
        case 'tool_use':s.toolCount++;s.currentText='';
            // Detect new loop: if last event was tool_result (or first tool), it's a new loop
            if(s.lastStreamType!=='tool_use'){s.loopNum++;s.loopToolCount=0;
              if(isActive()){const container=ensureAssistantMessage();const hdr=document.createElement('div');hdr.className='loop-header';hdr.textContent='\u{1F504} loop '+s.loopNum;container.appendChild(hdr);}
            }
            s.loopToolCount++;s.lastStreamType='tool_use';
            if(isActive())addToolCall(e.tool_name,e.input,e.tool_use_id);break;
        case 'tool_result':s.lastStreamType='tool_result';if(isActive())addToolResult(e.tool_use_id,e.output,e.is_error);break;
        case 'permission_request':if(isActive())addPermissionRequest(e.tool_name,e.input,e.tool_use_id);break;
        case 'result':if(e.usage)s.contextTokens=(e.usage.input_tokens||0)+(e.usage.output_tokens||0);if(e.total_cost_usd!==undefined)s.totalCost=e.total_cost_usd;if(isActive()){addStatsBar(e);updateSessionInfo(tab,cwd);}s.currentText='';s.isStreaming=false;s.toolCount=0;s.currentAssistantEl=null;s._currentTextEl=null;s.queryStartTime=0;s.loopNum=0;s.loopToolCount=0;s.lastStreamType='';if(isActive())setBusy(false);break;
        case 'error':if(isActive()){ensureAssistantMessage().innerHTML+='<div style="color:var(--red)">\u2717 ['+(e.code||'Error')+'] '+esc(e.message||'')+'</div>';}
          s.currentText='';s.isStreaming=false;s.currentAssistantEl=null;s.queryStartTime=0;if(isActive())setBusy(false);break;
        case 'state_update':{const p=e.patch||e;if(p.usage){s.contextTokens=(p.usage.input_tokens||0)+(p.usage.output_tokens||0);}if(p.total_cost_usd!==undefined)s.totalCost=p.total_cost_usd;if(isActive())updateSessionInfo(tab,cwd);break;}
          case 'model_fallback':if(isActive())ensureAssistantMessage().innerHTML+='<div style="color:var(--yellow)">\u26A0 '+esc(e.from_model)+' \u2192 '+esc(e.to_model)+'</div>';break;
      }break;}
    case 'compactDone':{const r=msg.data;if(isActive())addSystemMessage('\u{1F5DC}\uFE0F Compacted: '+r.tokens_before.toLocaleString()+' \u2192 '+r.tokens_after.toLocaleString()+' tokens');break;}
    case 'rpcResult':{
      if(msg.method==='projectsList'&&isActive())renderProjects(msg.data.projects||[]);
      else if(msg.method==='talkTail'&&isActive()){
        const ms=msg.data.messages||[];if(!ms.length)break;
        const container=getActiveMsgEl();const isEmpty=container.querySelectorAll('.msg').length===0;
        if(isEmpty){let loopNum=0;
          for(const m of ms){const txt=(m.text||'').trim();const tools=m.tools||[];
          if(m.role==='user'){
            // Skip tool-result-only messages (they are part of the loop, not user input)
            if(!txt)continue;
            loopNum=0;addUserMessage(txt);
          }
          else if(m.role==='assistant'){
            s.currentAssistantEl=null;s._currentTextEl=null;
            const hasTools=tools.length>0;
            // If this assistant message has tools, it's a loop iteration
            if(hasTools){loopNum++;
              // Add loop header
              const container=ensureAssistantMessage();
              const hdr=document.createElement('div');hdr.style.cssText='font-size:11px;color:var(--text-dim);margin:4px 0;';hdr.textContent='\u{1F504} loop '+loopNum+' ('+tools.length+' tool'+(tools.length>1?'s':'')+')';
              container.appendChild(hdr);
              for(const t of tools){
                const tn=typeof t==='string'?t:(t.name||'?');
                const detail=typeof t==='object'&&t.detail?t.detail:'';
                const d=document.createElement('details');d.className='tool-call';
                const label=tn==='Bash'&&detail?'$ '+esc(detail):(['FileRead','Read','FileWrite','Write','FileEdit','Edit'].includes(tn)&&detail?esc(tn)+' '+esc(detail):esc(tn)+(detail?' '+esc(detail):''));
                d.innerHTML='<summary>\u26A1 '+label+'</summary><div class="tool-body" style="color:var(--text-dim)">'+(detail?esc(detail):'(no detail)')+'</div>';
                container.appendChild(d);
              }
            }
            if(txt){s.currentText=txt;ensureTextSegment();renderAssistantText();s.currentText='';}
            s.currentAssistantEl=null;s._currentTextEl=null;
          }
        }break;}
        let h='<div style="font-size:13px">\u{1F4DC} <b>History</b> ('+msg.data.count+'/'+msg.data.total+')</div><div style="margin-top:8px;border-left:2px solid var(--border);padding-left:10px">';
        for(const m of ms){const ts=m.timestamp?m.timestamp.slice(11,19):'';const txt=(m.text||'').trim();
          if(m.role==='user')h+='<div style="margin:6px 0"><span style="color:var(--text-dim);font-size:11px">'+ts+'</span> <b style="color:var(--text-bright)">You</b><div style="margin-top:2px;color:var(--text)">'+(txt?esc(txt.slice(0,200))+(txt.length>200?'\u2026':''):'(attachment)')+'</div></div>';
          else if(m.role==='assistant')h+='<div style="margin:6px 0"><span style="color:var(--text-dim);font-size:11px">'+ts+'</span> <b style="color:var(--accent)">BC</b>'+(m.tools&&m.tools.length?' <span style="color:var(--cyan);font-size:11px">\u26A1'+m.tools.length+'</span>':'')+'<div style="margin-top:2px;color:var(--text-dim)">'+(txt?esc(txt.slice(0,200))+(txt.length>200?'\u2026':''):(m.tools&&m.tools.length?m.tools.join(', '):'...'))+'</div></div>';
        }
        h+='</div>';addSystemMessage(h);}
      break;}
    case 'error':if(isActive())setStatus(msg.message,'error');break;
  }
}

inputEl.focus();
