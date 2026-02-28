import{r as n,j as r}from"./launchpad.js";import{dy as N,d4 as U,d5 as D,d6 as M,d9 as d,dH as W,du as A,dw as O,e3 as z,dr as B,dq as s}from"./launchpad-App-D179iYht.js";import{n as q}from"./launchpad-OpenLink-DZHy38vr-CYK5YBWn.js";import{C as P}from"./launchpad-QrCode-VCBMkqqq-DYkl2ssX.js";import{$ as V}from"./launchpad-ModalHeader-JjfRejxC-gvXxgADS.js";import{r as H}from"./launchpad-LabelXs-oqZNqbm_-D2WTgiPX.js";import{a as Q}from"./launchpad-shouldProceedtoEmbeddedWalletCreationFlow-BsPl2jCD-BqPojStS.js";import{n as J}from"./launchpad-ScreenLayout-Ca6ml9wY-CeAdm0Km.js";import{l as F}from"./launchpad-farcaster-DPlSjvF5-CknmVXgZ.js";import{C as K}from"./launchpad-check-C5kCRahI.js";import{C as X}from"./launchpad-copy-DYa4Op8P.js";import"./launchpad-dijkstra-D_NXgYpA.js";import"./launchpad-Screen-DE3ldE_X-BBf8NvvX.js";import"./launchpad-index-Dq_xe9dz-bxbF5Mdc.js";import"./launchpad-createLucideIcon-BSOjrqRH.js";let Y=s.div`
  width: 100%;
`,G=s.div`
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.75rem;
  padding: 0.75rem;
  height: 56px;
  background: ${e=>e.$disabled?"var(--privy-color-background-2)":"var(--privy-color-background)"};
  border: 1px solid var(--privy-color-foreground-4);
  border-radius: var(--privy-border-radius-md);

  &:hover {
    border-color: ${e=>e.$disabled?"var(--privy-color-foreground-4)":"var(--privy-color-foreground-3)"};
  }
`,Z=s.div`
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
`,$=s.span`
  display: block;
  font-size: 16px;
  line-height: 24px;
  color: ${e=>e.$disabled?"var(--privy-color-foreground-2)":"var(--privy-color-foreground)"};
  overflow: hidden;
  text-overflow: ellipsis;
  /* Use single-line truncation without nowrap to respect container width */
  display: -webkit-box;
  -webkit-line-clamp: 1;
  -webkit-box-orient: vertical;
  word-break: break-all;

  @media (min-width: 441px) {
    font-size: 14px;
    line-height: 20px;
  }
`,ee=s($)`
  color: var(--privy-color-foreground-3);
  font-style: italic;
`,re=s(H)`
  margin-bottom: 0.5rem;
`,te=s(V)`
  && {
    gap: 0.375rem;
    font-size: 14px;
    flex-shrink: 0;
  }
`;const ae=({value:e,title:u,placeholder:l,className:t,showCopyButton:c=!0,truncate:o,maxLength:p=40,disabled:m=!1})=>{let[h,x]=n.useState(!1),w=o&&e?((a,S,f)=>{if((a=a.startsWith("https://")?a.slice(8):a).length<=f)return a;if(S==="middle"){let y=Math.ceil(f/2)-2,E=Math.floor(f/2)-1;return`${a.slice(0,y)}...${a.slice(-E)}`}return`${a.slice(0,f-3)}...`})(e,o,p):e;return n.useEffect(()=>{if(h){let a=setTimeout(()=>x(!1),3e3);return()=>clearTimeout(a)}},[h]),r.jsxs(Y,{className:t,children:[u&&r.jsx(re,{children:u}),r.jsxs(G,{$disabled:m,children:[r.jsx(Z,{children:e?r.jsx($,{$disabled:m,title:e,children:w}):r.jsx(ee,{$disabled:m,children:l||"No value"})}),c&&e&&r.jsx(te,{onClick:function(a){a.stopPropagation(),navigator.clipboard.writeText(e).then(()=>x(!0)).catch(console.error)},size:"sm",children:r.jsxs(r.Fragment,h?{children:["Copied",r.jsx(K,{size:14})]}:{children:["Copy",r.jsx(X,{size:14})]})})]})]})},ie=({connectUri:e,loading:u,success:l,errorMessage:t,onBack:c,onClose:o,onOpenFarcaster:p})=>r.jsx(J,O||u?z?{title:t?t.message:"Sign in with Farcaster",subtitle:t?t.detail:"To sign in with Farcaster, please open the Farcaster app.",icon:F,iconVariant:"loading",iconLoadingStatus:{success:l,fail:!!t},primaryCta:e&&p?{label:"Open Farcaster app",onClick:p}:void 0,onBack:c,onClose:o,watermark:!0}:{title:t?t.message:"Signing in with Farcaster",subtitle:t?t.detail:"This should only take a moment",icon:F,iconVariant:"loading",iconLoadingStatus:{success:l,fail:!!t},onBack:c,onClose:o,watermark:!0,children:e&&O&&r.jsx(oe,{children:r.jsx(q,{text:"Take me to Farcaster",url:e,color:"#8a63d2"})})}:{title:"Sign in with Farcaster",subtitle:"Scan with your phone's camera to continue.",onBack:c,onClose:o,watermark:!0,children:r.jsxs(se,{children:[r.jsx(ne,{children:e?r.jsx(P,{url:e,size:275,squareLogoElement:F}):r.jsx(de,{children:r.jsx(B,{})})}),r.jsxs(le,{children:[r.jsx(ce,{children:"Or copy this link and paste it into a phone browser to open the Farcaster app."}),e&&r.jsx(ae,{value:e,truncate:"end",maxLength:30,showCopyButton:!0,disabled:!0})]})]})}),je={component:()=>{let{authenticated:e,logout:u,ready:l,user:t}=N(),{lastScreen:c,navigate:o,navigateBack:p,setModalData:m}=U(),h=D(),{getAuthFlow:x,loginWithFarcaster:w,closePrivyModal:a,createAnalyticsEvent:S}=M(),[f,y]=n.useState(void 0),[E,I]=n.useState(!1),[b,R]=n.useState(!1),C=n.useRef([]),_=x(),T=_?.meta.connectUri;return n.useEffect(()=>{let g=Date.now(),j=setInterval(async()=>{let k=await _.pollForReady.execute(),L=Date.now()-g;if(k){clearInterval(j),I(!0);try{await w(),R(!0)}catch(i){let v={retryable:!1,message:"Authentication failed"};if(i?.privyErrorCode===d.ALLOWLIST_REJECTED)return void o("AllowlistRejectionScreen");if(i?.privyErrorCode===d.USER_LIMIT_REACHED)return console.error(new W(i).toString()),void o("UserLimitReachedScreen");if(i?.privyErrorCode===d.USER_DOES_NOT_EXIST)return void o("AccountNotFoundScreen");if(i?.privyErrorCode===d.LINKED_TO_ANOTHER_USER)v.detail=i.message??"This account has already been linked to another user.";else{if(i?.privyErrorCode===d.ACCOUNT_TRANSFER_REQUIRED&&i.data?.data?.nonce)return m({accountTransfer:{nonce:i.data?.data?.nonce,account:i.data?.data?.subject,displayName:i.data?.data?.account?.displayName,linkMethod:"farcaster",embeddedWalletAddress:i.data?.data?.otherUser?.embeddedWalletAddress,farcasterEmbeddedAddress:i.data?.data?.otherUser?.farcasterEmbeddedAddress}}),void o("LinkConflictScreen");i?.privyErrorCode===d.INVALID_CREDENTIALS?(v.retryable=!0,v.detail="Something went wrong. Try again."):i?.privyErrorCode===d.TOO_MANY_REQUESTS&&(v.detail="Too many requests. Please wait before trying again.")}y(v)}}else L>12e4&&(clearInterval(j),y({retryable:!0,message:"Authentication failed",detail:"The request timed out. Try again."}))},2e3);return()=>{clearInterval(j),C.current.forEach(k=>clearTimeout(k))}},[]),n.useEffect(()=>{if(l&&e&&b&&t){if(h?.legal.requireUsersAcceptTerms&&!t.hasAcceptedTerms){let g=setTimeout(()=>{o("AffirmativeConsentScreen")},A);return()=>clearTimeout(g)}b&&(Q(t,h.embeddedWallets)?C.current.push(setTimeout(()=>{m({createWallet:{onSuccess:()=>{},onFailure:g=>{console.error(g),S({eventName:"embedded_wallet_creation_failure_logout",payload:{error:g,screen:"FarcasterConnectStatusScreen"}}),u()},callAuthOnSuccessOnClose:!0}}),o("EmbeddedWalletOnAccountCreateScreen")},A)):C.current.push(setTimeout(()=>a({shouldCallAuthOnSuccess:!0,isSuccess:!0}),A)))}},[b,l,e,t]),r.jsx(ie,{connectUri:T,loading:E,success:b,errorMessage:f,onBack:c?p:void 0,onClose:a,onOpenFarcaster:()=>{T&&(window.location.href=T)}})}};let oe=s.div`
  margin-top: 24px;
`,se=s.div`
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 24px;
`,ne=s.div`
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 275px;
`,le=s.div`
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
`,ce=s.div`
  font-size: 0.875rem;
  text-align: center;
  color: var(--privy-color-foreground-2);
`,de=s.div`
  position: relative;
  width: 82px;
  height: 82px;
`;export{je as FarcasterConnectStatusScreen,ie as FarcasterConnectStatusView,je as default};
