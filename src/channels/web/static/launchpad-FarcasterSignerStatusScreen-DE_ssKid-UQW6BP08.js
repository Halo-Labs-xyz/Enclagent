import{r as c,j as t}from"./launchpad.js";import{d4 as F,d5 as T,d6 as I,du as w,dw as y,e3 as q,dr as O,dq as o}from"./launchpad-App-CJN8YHj9.js";import{h as _}from"./launchpad-CopyToClipboard-DSTf_eKU-C5attl0r.js";import{n as B}from"./launchpad-OpenLink-DZHy38vr-Dmk-sJBI.js";import{C as E}from"./launchpad-QrCode-VCBMkqqq-CccYvGLK.js";import{n as A}from"./launchpad-ScreenLayout-Ca6ml9wY-tWBwodfe.js";import{l as h}from"./launchpad-farcaster-DPlSjvF5-CknmVXgZ.js";import"./launchpad-dijkstra-D_NXgYpA.js";import"./launchpad-ModalHeader-JjfRejxC-CtYe7KE2.js";import"./launchpad-Screen-DE3ldE_X-Bg_Q5Z_h.js";import"./launchpad-index-Dq_xe9dz-DPq0PTtR.js";let S="#8a63d2";const M=({appName:u,loading:m,success:d,errorMessage:e,connectUri:a,onBack:r,onClose:l,onOpenFarcaster:s})=>t.jsx(A,y||m?q?{title:e?e.message:"Add a signer to Farcaster",subtitle:e?e.detail:`This will allow ${u} to add casts, likes, follows, and more on your behalf.`,icon:h,iconVariant:"loading",iconLoadingStatus:{success:d,fail:!!e},primaryCta:a&&s?{label:"Open Farcaster app",onClick:s}:void 0,onBack:r,onClose:l,watermark:!0}:{title:e?e.message:"Requesting signer from Farcaster",subtitle:e?e.detail:"This should only take a moment",icon:h,iconVariant:"loading",iconLoadingStatus:{success:d,fail:!!e},onBack:r,onClose:l,watermark:!0,children:a&&y&&t.jsx(P,{children:t.jsx(B,{text:"Take me to Farcaster",url:a,color:S})})}:{title:"Add a signer to Farcaster",subtitle:`This will allow ${u} to add casts, likes, follows, and more on your behalf.`,onBack:r,onClose:l,watermark:!0,children:t.jsxs(R,{children:[t.jsx(L,{children:a?t.jsx(E,{url:a,size:275,squareLogoElement:h}):t.jsx(z,{children:t.jsx(O,{})})}),t.jsxs(N,{children:[t.jsx(V,{children:"Or copy this link and paste it into a phone browser to open the Farcaster app."}),a&&t.jsx(_,{text:a,itemName:"link",color:S})]})]})});let P=o.div`
  margin-top: 24px;
`,R=o.div`
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 24px;
`,L=o.div`
  padding: 24px;
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 275px;
`,N=o.div`
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
`,V=o.div`
  font-size: 0.875rem;
  text-align: center;
  color: var(--privy-color-foreground-2);
`,z=o.div`
  position: relative;
  width: 82px;
  height: 82px;
`;const $={component:()=>{let{lastScreen:u,navigateBack:m,data:d}=F(),e=T(),{requestFarcasterSignerStatus:a,closePrivyModal:r}=I(),[l,s]=c.useState(void 0),[k,x]=c.useState(!1),[j,v]=c.useState(!1),g=c.useRef([]),n=d?.farcasterSigner;c.useEffect(()=>{let C=Date.now(),i=setInterval(async()=>{if(!n?.public_key)return clearInterval(i),void s({retryable:!0,message:"Connect failed",detail:"Something went wrong. Please try again."});n.status==="approved"&&(clearInterval(i),x(!1),v(!0),g.current.push(setTimeout(()=>r({shouldCallAuthOnSuccess:!1,isSuccess:!0}),w)));let p=await a(n?.public_key),b=Date.now()-C;p.status==="approved"?(clearInterval(i),x(!1),v(!0),g.current.push(setTimeout(()=>r({shouldCallAuthOnSuccess:!1,isSuccess:!0}),w))):b>3e5?(clearInterval(i),s({retryable:!0,message:"Connect failed",detail:"The request timed out. Try again."})):p.status==="revoked"&&(clearInterval(i),s({retryable:!0,message:"Request rejected",detail:"The request was rejected. Please try again."}))},2e3);return()=>{clearInterval(i),g.current.forEach(p=>clearTimeout(p))}},[]);let f=n?.status==="pending_approval"?n.signer_approval_url:void 0;return t.jsx(M,{appName:e.name,loading:k,success:j,errorMessage:l,connectUri:f,onBack:u?m:void 0,onClose:r,onOpenFarcaster:()=>{f&&(window.location.href=f)}})}};export{$ as FarcasterSignerStatusScreen,M as FarcasterSignerStatusView,$ as default};
