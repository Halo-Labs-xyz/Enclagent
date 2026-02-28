import{r as o,j as t}from"./launchpad.js";import{dy as A,d6 as I,d4 as M,eX as N,dE as E,es as C,et as T,du as k,dq as p,ci as O,cf as $,eY as q}from"./launchpad-App-CJN8YHj9.js";import{h as z}from"./launchpad-CopyToClipboard-DSTf_eKU-C5attl0r.js";import{a as P}from"./launchpad-Layouts-BlFm53ED-C7q8tWPJ.js";import{a as F,i as V}from"./launchpad-JsonTree-aPaJmPx7-Cy5MSHnf.js";import{n as H}from"./launchpad-ScreenLayout-Ca6ml9wY-tWBwodfe.js";import{c as J}from"./launchpad-createLucideIcon-BSOjrqRH.js";import"./launchpad-ModalHeader-JjfRejxC-CtYe7KE2.js";import"./launchpad-Screen-DE3ldE_X-Bg_Q5Z_h.js";import"./launchpad-index-Dq_xe9dz-DPq0PTtR.js";/**
 * @license lucide-react v0.554.0 - ISC
 *
 * This source code is licensed under the ISC license.
 * See the LICENSE file in the root directory of this source tree.
 */const W=[["path",{d:"M12 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7",key:"1m0v6g"}],["path",{d:"M18.375 2.625a1 1 0 0 1 3 3l-9.013 9.014a2 2 0 0 1-.853.505l-2.873.84a.5.5 0 0 1-.62-.62l.84-2.873a2 2 0 0 1 .506-.852z",key:"ohrbg2"}]],B=J("square-pen",W),K=p.img`
  && {
    height: ${e=>e.size==="sm"?"65px":"140px"};
    width: ${e=>e.size==="sm"?"65px":"140px"};
    border-radius: 16px;
    margin-bottom: 12px;
  }
`;let Q=e=>{if(!O(e))return e;try{let a=$(e);return a.includes("�")?e:a}catch{return e}},X=e=>{try{let a=q.decode(e),s=new TextDecoder().decode(a);return s.includes("�")?e:s}catch{return e}},Y=e=>{let{types:a,primaryType:s,...l}=e.typedData;return t.jsxs(t.Fragment,{children:[t.jsx(te,{data:l}),t.jsx(z,{text:(n=e.typedData,JSON.stringify(n,null,2)),itemName:"full payload to clipboard"})," "]});var n};const G=({method:e,messageData:a,copy:s,iconUrl:l,isLoading:n,success:g,walletProxyIsLoading:m,errorMessage:x,isCancellable:d,onSign:c,onCancel:y,onClose:u})=>t.jsx(H,{title:s.title,subtitle:s.description,showClose:!0,onClose:u,icon:B,iconVariant:"subtle",helpText:x?t.jsx(ee,{children:x}):void 0,primaryCta:{label:s.buttonText,onClick:c,disabled:n||g||m,loading:n},secondaryCta:d?{label:"Not now",onClick:y,disabled:n||g||m}:void 0,watermark:!0,children:t.jsxs(P,{children:[l?t.jsx(K,{style:{alignSelf:"center"},size:"sm",src:l,alt:"app image"}):null,t.jsxs(Z,{children:[e==="personal_sign"&&t.jsx(w,{children:Q(a)}),e==="eth_signTypedData_v4"&&t.jsx(Y,{typedData:a}),e==="solana_signMessage"&&t.jsx(w,{children:X(a)})]})]})}),pe={component:()=>{let{authenticated:e}=A(),{initializeWalletProxy:a,closePrivyModal:s}=I(),{navigate:l,data:n,onUserCloseViaDialogOrKeybindRef:g}=M(),[m,x]=o.useState(!0),[d,c]=o.useState(""),[y,u]=o.useState(),[f,b]=o.useState(null),[R,S]=o.useState(!1);o.useEffect(()=>{e||l("LandingScreen")},[e]),o.useEffect(()=>{a(N).then(i=>{x(!1),i||(c("An error has occurred, please try again."),u(new E(new C(d,T.E32603_DEFAULT_INTERNAL_ERROR.eipCode))))})},[]);let{method:_,data:j,confirmAndSign:v,onSuccess:D,onFailure:L,uiOptions:r}=n.signMessage,U={title:r?.title||"Sign message",description:r?.description||"Signing this message will not cost you any fees.",buttonText:r?.buttonText||"Sign and continue"},h=i=>{i?D(i):L(y||new E(new C("The user rejected the request.",T.E4001_USER_REJECTED_REQUEST.eipCode))),s({shouldCallAuthOnSuccess:!1}),setTimeout(()=>{b(null),c(""),u(void 0)},200)};return g.current=()=>{h(f)},t.jsx(G,{method:_,messageData:j,copy:U,iconUrl:r?.iconUrl&&typeof r.iconUrl=="string"?r.iconUrl:void 0,isLoading:R,success:f!==null,walletProxyIsLoading:m,errorMessage:d,isCancellable:r?.isCancellable,onSign:async()=>{S(!0),c("");try{let i=await v();b(i),S(!1),setTimeout(()=>{h(i)},k)}catch(i){console.error(i),c("An error has occurred, please try again."),u(new E(new C(d,T.E32603_DEFAULT_INTERNAL_ERROR.eipCode))),S(!1)}},onCancel:()=>h(null),onClose:()=>h(f)})}};let Z=p.div`
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 16px;
`,ee=p.p`
  && {
    margin: 0;
    width: 100%;
    text-align: center;
    color: var(--privy-color-error-dark);
    font-size: 14px;
    line-height: 22px;
  }
`,te=p(F)`
  margin-top: 0;
`,w=p(V)`
  margin-top: 0;
`;export{pe as SignRequestScreen,G as SignRequestView,pe as default};
