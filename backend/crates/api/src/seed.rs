use crate::config::SeedProfile;
use babel_authoring::ObjectDraft;
use babel_graph::{EdgeOrigin, Relation};
use babel_identity::IdentityKind;
use babel_judgment::JudgmentProvider;
use babel_media::MediaBlob;
use babel_node::{LocalNode, ObjectSearchQuery};
use babel_object::{Object, Surface, SurfaceRole, SurfaceTarget};
use babel_types::{IdentityId, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SeedReport {
    pub profile: SeedProfile,
    pub inserted_objects: usize,
}

pub fn apply_seed_profile<P>(
    node: &mut LocalNode<P>,
    profile: SeedProfile,
    public_origin: &str,
) -> Result<SeedReport>
where
    P: JudgmentProvider,
{
    match profile {
        SeedProfile::CardFeed => seed_card_feed(node, public_origin),
    }
}

fn seed_card_feed<P>(node: &mut LocalNode<P>, public_origin: &str) -> Result<SeedReport>
where
    P: JudgmentProvider,
{
    if !node
        .search_objects(ObjectSearchQuery {
            query: None,
            author: None,
            kind: None,
            limit: 1,
        })?
        .is_empty()
    {
        return Ok(SeedReport {
            profile: SeedProfile::CardFeed,
            inserted_objects: 0,
        });
    }

    let author = node.create_identity(IdentityKind::Agent, "babel-local-seed")?;
    let surface = put_surface_bundle(node, public_origin)?;
    let objects = publish_feed_objects(node, &author.id, &surface)?;
    publish_edges(node, &author.id, &objects)?;

    Ok(SeedReport {
        profile: SeedProfile::CardFeed,
        inserted_objects: objects.len(),
    })
}

fn publish_feed_objects<P>(
    node: &mut LocalNode<P>,
    author_id: &IdentityId,
    surface: &SurfaceBundle,
) -> Result<Vec<Object>>
where
    P: JudgmentProvider,
{
    [
        "Executable Objects should make social posts feel inspectable, forkable, and alive.",
        "Discovery is a graph problem first. Ranking comes after evidence and relationships are visible.",
        "Lenses belong to users. A feed should explain why it chose a path through the network.",
        "Hashgraph finality is reserved for shared ordering, not every local thought.",
        "Surfaces turn an Object into software without letting it inherit the host's authority.",
    ]
    .into_iter()
    .map(|text| {
        let draft = ObjectDraft::text(text)?
            .with_resource(surface.html_blob.resource())?
            .with_resource(surface.script_blob.resource())?
            .with_surface(surface.surface.clone())?;
        node.publish_draft(author_id, draft)
    })
    .collect()
}

fn publish_edges<P>(
    node: &mut LocalNode<P>,
    author_id: &IdentityId,
    objects: &[Object],
) -> Result<()>
where
    P: JudgmentProvider,
{
    for pair in objects.windows(2) {
        let source = pair[0].id.clone();
        let target = pair[1].id.clone();
        node.publish_edge(
            author_id,
            source,
            target,
            Relation::References,
            EdgeOrigin::ApplicationAssertion,
        )?;
    }
    Ok(())
}

fn put_surface_bundle<P>(node: &LocalNode<P>, public_origin: &str) -> Result<SurfaceBundle>
where
    P: JudgmentProvider,
{
    let script = surface_script();
    let script_blob = node.put_media_blob("text/javascript", script.as_bytes())?;
    let script_url = blob_url(public_origin, &script_blob);
    let html = surface_html(&script_url);
    let html_blob = node.put_media_blob("text/html", html.as_bytes())?;
    let surface = Surface {
        bundle: None,
        role: SurfaceRole::Feed,
        target: SurfaceTarget::Web,
        entry: blob_url(public_origin, &html_blob),
        integrity: Some(html_blob.integrity.clone()),
    };
    Ok(SurfaceBundle {
        html_blob,
        script_blob,
        surface,
    })
}

fn blob_url(public_origin: &str, blob: &MediaBlob) -> String {
    format!(
        "{}/runtime/surfaces/blobs/{}?media_type={}",
        public_origin.trim_end_matches('/'),
        blob.integrity,
        blob.media_type
    )
}

fn surface_html(script_url: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Babel Surface</title></head><body><main><h1>Babel Object Surface</h1><p>This Surface is loaded from a content-addressed Object resource.</p><button type=\"button\" id=\"ping\">Ping host RPC bridge</button><pre id=\"output\">ready</pre></main><script src=\"{script_url}\"></script></body></html>"
    )
}

fn surface_script() -> String {
    r#"const output=document.getElementById('output');
const button=document.getElementById('ping');
let sequence=0;
let connected=false;
let accepted=false;
let closed=false;
const channel=new MessageChannel();
const control=type=>({type:`babel.surface.${type}`,protocol:'babel.rpc.v1',version:1});
button.disabled=true;
const handshake=setTimeout(()=>closeBridge('Secure bridge unavailable. Reopen this Surface.'),10000);
function closeBridge(message){
  if(closed)return;
  closed=true;
  connected=false;
  clearTimeout(handshake);
  button.disabled=true;
  output.textContent=message;
  try{channel.port1.postMessage(control('close'));}finally{channel.port1.close();}
}
function sendBridgeRequest(id){
  if(!connected||closed)return;
  output.textContent=`sent ${id}`;
  channel.port1.postMessage({
    type:'babel.rpc.request',
    protocol:'babel.rpc.v1',
    envelope:{
      protocol:'babel.rpc.v1',
      id,
      method:'babel.search.objects.v1',
      binding:{
        object_id:null,
        surface_session_id:null,
        runtime_id:'seed-surface',
        origin:location.origin,
        capability_grants:[]
      },
      payload:{q:null,author:null,kind:null,limit:1},
      idempotency_key:null,
      deadline:{timeout_ms:30000,client_started_at:new Date().toISOString()},
      trace_id:null
    }
  });
}
button?.addEventListener('click',()=>sendBridgeRequest(`surface-ping-${++sequence}`));
channel.port1.onmessage=(event)=>{
  const data=event.data;
  if(data?.protocol!=='babel.rpc.v1')return;
  if(data?.version===1&&Object.keys(data).length===3){
    if(data.type==='babel.surface.accept'&&!accepted&&!connected&&!closed){
      accepted=true;
      channel.port1.postMessage(control('confirm'));
    }else if(data.type==='babel.surface.ready'&&accepted&&!connected&&!closed){
      clearTimeout(handshake);
      connected=true;
      button.disabled=false;
      sendBridgeRequest('surface-auto-1');
    }else if(data.type==='babel.surface.close')closeBridge('Surface bridge closed');
    return;
  }
  if(data?.type==='babel.rpc.response'&&data?.response?.id){
    if(data.response.error){output.textContent=data.response.error.message;return;}
    const count=data.response.result?.results?.length??0;
    output.textContent=`received ${data.response.id} results ${count}`;
  }
};
channel.port1.onmessageerror=()=>closeBridge('Surface bridge could not decode a message');
window.addEventListener('pagehide',()=>closeBridge('Surface document closed'),{once:true});
// The offer contains no credentials; RPC and replies never use Window messaging.
try{window.parent.postMessage(control('connect'),'*',[channel.port2]);}
catch{closeBridge('Surface bridge could not connect');}"#
        .to_string()
}

struct SurfaceBundle {
    html_blob: MediaBlob,
    script_blob: MediaBlob,
    surface: Surface,
}
