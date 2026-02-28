import{r as s,j as r}from"./launchpad.js";import{dy as B,d6 as S,d5 as W,d4 as L,dq as c,dQ as U}from"./launchpad-App-CJN8YHj9.js";import{t as $}from"./launchpad-WarningBanner-c8L53pJ2-Di6f77Un.js";import{j as R}from"./launchpad-WalletInfoCard-CuZCmvHw-CXLpfV91.js";import{n as z}from"./launchpad-ScreenLayout-Ca6ml9wY-tWBwodfe.js";import"./launchpad-ExclamationTriangleIcon-DS9jnCFw.js";import"./launchpad-ModalHeader-JjfRejxC-CtYe7KE2.js";import"./launchpad-ErrorMessage-D8VaAP5m-BxUDDoNF.js";import"./launchpad-LabelXs-oqZNqbm_-UAiqAEGh.js";import"./launchpad-Address-D-q_5it9-CEPVIheL.js";import"./launchpad-check-C5kCRahI.js";import"./launchpad-createLucideIcon-BSOjrqRH.js";import"./launchpad-copy-DYa4Op8P.js";import"./launchpad-shared-FM0rljBt-DH-prbl1.js";import"./launchpad-Screen-DE3ldE_X-Bg_Q5Z_h.js";import"./launchpad-index-Dq_xe9dz-DPq0PTtR.js";const K=({address:e,accessToken:t,appConfigTheme:a,onClose:l,isLoading:d=!1,exportButtonProps:i,onBack:n})=>r.jsx(z,{title:"Export wallet",subtitle:r.jsxs(r.Fragment,{children:["Copy either your private key or seed phrase to export your wallet."," ",r.jsx("a",{href:"https://privy-io.notion.site/Transferring-your-account-9dab9e16c6034a7ab1ff7fa479b02828",target:"blank",rel:"noopener noreferrer",children:"Learn more"})]}),onClose:l,onBack:n,showBack:!!n,watermark:!0,children:r.jsxs(O,{children:[r.jsx($,{theme:a,children:"Never share your private key or seed phrase with anyone."}),r.jsx(R,{title:"Your wallet",address:e,showCopyButton:!0}),r.jsx("div",{style:{width:"100%"},children:d?r.jsx(q,{}):t&&i&&r.jsx(N,{accessToken:t,dimensions:{height:"44px"},...i})})]})});let O=c.div`
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
  text-align: left;
`,q=()=>r.jsx(D,{children:r.jsx(F,{children:"Loading..."})}),D=c.div`
  display: flex;
  gap: 12px;
  height: 44px;
`,F=c.div`
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 100%;
  font-size: 16px;
  font-weight: 500;
  border-radius: var(--privy-border-radius-md);
  background-color: var(--privy-color-background-2);
  color: var(--privy-color-foreground-3);
`;function N(e){let[t,a]=s.useState(e.dimensions.width),[l,d]=s.useState(void 0),i=s.useRef(null);s.useEffect(()=>{if(i.current&&t===void 0){let{width:p}=i.current.getBoundingClientRect();a(p)}let o=getComputedStyle(document.documentElement);d({background:o.getPropertyValue("--privy-color-background"),background2:o.getPropertyValue("--privy-color-background-2"),foreground3:o.getPropertyValue("--privy-color-foreground-3"),foregroundAccent:o.getPropertyValue("--privy-color-foreground-accent"),accent:o.getPropertyValue("--privy-color-accent"),accentDark:o.getPropertyValue("--privy-color-accent-dark"),success:o.getPropertyValue("--privy-color-success"),colorScheme:o.getPropertyValue("color-scheme")})},[]);let n=e.chainType==="ethereum"&&!e.imported&&!e.isUnifiedWallet;return r.jsx("div",{ref:i,children:t&&r.jsxs(M,{children:[r.jsx("iframe",{style:{position:"absolute",zIndex:1},width:t,height:e.dimensions.height,allow:"clipboard-write self *",src:U({origin:e.origin,path:`/apps/${e.appId}/embedded-wallets/export`,query:e.isUnifiedWallet?{v:"1-unified",wallet_id:e.walletId,client_id:e.appClientId,width:`${t}px`,caid:e.clientAnalyticsId,phrase_export:n,...l}:{v:"1",entropy_id:e.entropyId,entropy_id_verifier:e.entropyIdVerifier,hd_wallet_index:e.hdWalletIndex,chain_type:e.chainType,client_id:e.appClientId,width:`${t}px`,caid:e.clientAnalyticsId,phrase_export:n,...l},hash:{token:e.accessToken}})}),r.jsx(g,{children:"Loading..."}),n&&r.jsx(g,{children:"Loading..."})]})})}const se={component:()=>{let[e,t]=s.useState(null),{authenticated:a,user:l}=B(),{closePrivyModal:d,createAnalyticsEvent:i,clientAnalyticsId:n,client:o}=S(),p=W(),{data:m,onUserCloseViaDialogOrKeybindRef:x}=L(),{onFailure:v,onSuccess:w,origin:b,appId:k,appClientId:I,entropyId:j,entropyIdVerifier:C,walletId:_,hdWalletIndex:V,chainType:E,address:y,isUnifiedWallet:T,imported:P,showBackButton:A}=m.keyExport,f=h=>{d({shouldCallAuthOnSuccess:!1}),v(typeof h=="string"?Error(h):h)},u=()=>{d({shouldCallAuthOnSuccess:!1}),w(),i({eventName:"embedded_wallet_key_export_completed",payload:{walletAddress:y}})};return s.useEffect(()=>{if(!a)return f("User must be authenticated before exporting their wallet");o.getAccessToken().then(t).catch(f)},[a,l]),x.current=u,r.jsx(K,{address:y,accessToken:e,appConfigTheme:p.appearance.palette.colorScheme,onClose:u,isLoading:!e,onBack:A?u:void 0,exportButtonProps:e?{origin:b,appId:k,appClientId:I,clientAnalyticsId:n,entropyId:j,entropyIdVerifier:C,walletId:_,hdWalletIndex:V,isUnifiedWallet:T,imported:P,chainType:E}:void 0})}};let M=c.div`
  overflow: visible;
  position: relative;
  overflow: none;
  height: 44px;
  display: flex;
  gap: 12px;
`,g=c.div`
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 100%;
  font-size: 16px;
  font-weight: 500;
  border-radius: var(--privy-border-radius-md);
  background-color: var(--privy-color-background-2);
  color: var(--privy-color-foreground-3);
`;export{se as EmbeddedWalletKeyExportScreen,K as EmbeddedWalletKeyExportView,se as default};
