import{r as g,j as e}from"./launchpad.js";import{dy as F,d4 as D,fJ as K,dB as A,fK as L,fL as W,dw as I,dq as m}from"./launchpad-App-D179iYht.js";import{p as V}from"./launchpad-CopyableText-BCytXyJL-C5DGCEOV.js";import{n as k}from"./launchpad-ScreenLayout-Ca6ml9wY-CeAdm0Km.js";import{i as P}from"./launchpad-InfoBanner-DkQEPd77-BJw8olDz.js";import{c as S}from"./launchpad-createLucideIcon-BSOjrqRH.js";import{C as R}from"./launchpad-check-C5kCRahI.js";import{C as $}from"./launchpad-circle-x-DddvhjMS.js";import"./launchpad-copy-DYa4Op8P.js";import"./launchpad-ModalHeader-JjfRejxC-gvXxgADS.js";import"./launchpad-Screen-DE3ldE_X-BBf8NvvX.js";import"./launchpad-index-Dq_xe9dz-bxbF5Mdc.js";/**
 * @license lucide-react v0.554.0 - ISC
 *
 * This source code is licensed under the ISC license.
 * See the LICENSE file in the root directory of this source tree.
 */const q=[["path",{d:"m6 9 6 6 6-6",key:"qrunsl"}]],N=S("chevron-down",q);/**
 * @license lucide-react v0.554.0 - ISC
 *
 * This source code is licensed under the ISC license.
 * See the LICENSE file in the root directory of this source tree.
 */const Y=[["path",{d:"M5 22h14",key:"ehvnwv"}],["path",{d:"M5 2h14",key:"pdyrp9"}],["path",{d:"M17 22v-4.172a2 2 0 0 0-.586-1.414L12 12l-4.414 4.414A2 2 0 0 0 7 17.828V22",key:"1d314k"}],["path",{d:"M7 2v4.172a2 2 0 0 0 .586 1.414L12 12l4.414-4.414A2 2 0 0 0 17 6.172V2",key:"1vvvr6"}]],O=S("hourglass",Y);/**
 * @license lucide-react v0.554.0 - ISC
 *
 * This source code is licensed under the ISC license.
 * See the LICENSE file in the root directory of this source tree.
 */const H=[["path",{d:"m16 11 2 2 4-4",key:"9rsbq5"}],["path",{d:"M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2",key:"1yyitq"}],["circle",{cx:"9",cy:"7",r:"4",key:"nufk8"}]],J=S("user-check",H),U=async({operation:t,until:a,delay:l,interval:p,attempts:h,signal:u})=>{let o,d=0;for(;d<h;){if(u?.aborted)return{status:"aborted",result:o,attempts:d};d++;try{if(o=await t(),a(o))return{status:"success",result:o,attempts:d};d<h&&await A(p)}catch{d<h&&await A(p)}}return{status:"max_attempts",result:o,attempts:d}},X=({data:t,onClose:a})=>e.jsx(k,{showClose:!0,onClose:a,title:"Initiate bank transfer",subtitle:"Use the details below to complete a bank transfer from your bank.",primaryCta:{label:"Done",onClick:a},watermark:!1,footerText:"Exchange rates and fees are set when you authorize and determine the amount you receive. You'll see the applicable rates and fees for your transaction separately",children:e.jsx(G,{children:(W[t.deposit_instructions.asset]||[]).map(([l,p],h)=>{let u=t.deposit_instructions[l];if(!u)return null;let o=l==="asset"?u.toUpperCase():u,d=o.length>100?`${o.slice(0,9)}...${o.slice(-9)}`:o;return e.jsxs(Q,{children:[e.jsx(Z,{children:p}),e.jsx(V,{value:o,includeChildren:I,children:e.jsx(ee,{children:d})})]},h)})})});let G=m.ol`
  border-color: var(--privy-color-border-default);
  border-width: 1px;
  border-radius: var(--privy-border-radius-mdlg);
  border-style: solid;
  display: flex;
  flex-direction: column;

  && {
    padding: 0 1rem;
  }
`,Q=m.li`
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 1rem 0;

  &:not(:first-of-type) {
    border-top: 1px solid var(--privy-color-border-default);
  }

  & > {
    :nth-child(1) {
      flex-basis: 30%;
    }

    :nth-child(2) {
      flex-basis: 60%;
    }
  }
`,Z=m.span`
  color: var(--privy-color-foreground);
  font-kerning: none;
  font-variant-numeric: lining-nums proportional-nums;
  font-feature-settings: 'calt' off;

  /* text-xs/font-regular */
  font-size: 0.75rem;
  font-style: normal;
  font-weight: 400;
  line-height: 1.125rem; /* 150% */

  text-align: left;
  flex-shrink: 0;
`,ee=m.span`
  color: var(--privy-color-foreground);
  font-kerning: none;
  font-feature-settings: 'calt' off;

  /* text-sm/font-medium */
  font-size: 0.875rem;
  font-style: normal;
  font-weight: 500;
  line-height: 1.375rem; /* 157.143% */

  text-align: right;
  word-break: break-all;
`;const te=({onClose:t})=>e.jsx(k,{showClose:!0,onClose:t,icon:$,iconVariant:"error",title:"Something went wrong",subtitle:"We couldn't complete account setup. This isn't caused by anything you did.",primaryCta:{label:"Close",onClick:t},watermark:!0}),re=({onClose:t,reason:a})=>{let l=a?a.charAt(0).toLowerCase()+a.slice(1):void 0;return e.jsx(k,{showClose:!0,onClose:t,icon:$,iconVariant:"error",title:"Identity verification failed",subtitle:l?`We can't complete identity verification because ${l}. Please try again or contact support for assistance.`:"We couldn't verify your identity. Please try again or contact support for assistance.",primaryCta:{label:"Close",onClick:t},watermark:!0})},oe=({onClose:t,email:a})=>e.jsx(k,{showClose:!0,onClose:t,icon:O,title:"Identity verification in progress",subtitle:"We're waiting for Persona to approve your identity verification. This usually takes a few minutes, but may take up to 24 hours.",primaryCta:{label:"Done",onClick:t},watermark:!0,children:e.jsxs(P,{theme:"light",children:["You'll receive an email at ",a," once approved with instructions for completing your deposit."]})}),se=({onClose:t,onAcceptTerms:a,isLoading:l})=>e.jsx(k,{showClose:!0,onClose:t,icon:J,title:"Verify your identity to continue",subtitle:"Finish verification with Persona — it takes just a few minutes and requires a government ID.",helpText:e.jsxs(e.Fragment,{children:[`This app uses Bridge to securely connect accounts and move funds. By clicking "Accept," you agree to Bridge's`," ",e.jsx("a",{href:"https://www.bridge.xyz/legal",target:"_blank",rel:"noopener noreferrer",children:"Terms of Service"})," ","and"," ",e.jsx("a",{href:"https://www.bridge.xyz/legal/row-privacy-policy/bridge-building-limited",target:"_blank",rel:"noopener noreferrer",children:"Privacy Policy"}),"."]}),primaryCta:{label:"Accept and continue",onClick:a,loading:l},watermark:!0}),ne=({onClose:t})=>e.jsx(k,{showClose:!0,onClose:t,icon:R,iconVariant:"success",title:"Identity verified successfully",subtitle:"We've successfully verified your identity. Now initiate a bank transfer to view instructions.",primaryCta:{label:"Initiate bank transfer",onClick:()=>{},loading:!0},watermark:!0}),ae=({opts:t,onClose:a,onEditSourceAsset:l,onSelectAmount:p,isLoading:h})=>{let{icon:u}=L[t.source.selectedAsset];return e.jsxs(k,{showClose:!0,onClose:a,headerTitle:`Buy ${t.destination.asset.toLocaleUpperCase()}`,primaryCta:{label:"Continue",onClick:p,loading:h},watermark:!0,children:[e.jsx(fe,{currency:t.source.selectedAsset,inputMode:"decimal",autoFocus:!0}),e.jsxs(ie,{onClick:l,children:[e.jsx(le,{children:u}),e.jsx(ce,{children:t.source.selectedAsset.toLocaleUpperCase()}),e.jsx(ue,{children:e.jsx(N,{})})]})]})};let ie=m.button`
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: auto;
  gap: 0.5rem;
  border: 1px solid var(--privy-color-border-default);
  border-radius: var(--privy-border-radius-full);

  && {
    margin: auto;
    padding: 0.5rem 1rem;
  }
`,le=m.div`
  svg {
    width: 1rem;
    height: 1rem;
    border-radius: var(--privy-border-radius-full);
    overflow: hidden;
  }
`,ce=m.span`
  color: var(--privy-color-foreground);
  font-kerning: none;
  font-feature-settings: 'calt' off;

  /* text-sm/font-medium */
  font-size: 0.875rem;
  font-style: normal;
  font-weight: 500;
  line-height: 1.375rem; /* 157.143% */
`,ue=m.div`
  color: var(--privy-color-foreground);

  svg {
    width: 1.25rem;
    height: 1.25rem;
  }
`,de={usd:"$",eur:"€",gbp:"£",mxn:"$",brl:"R$"},pe=m.span`
  background-color: var(--privy-color-background);
  width: 100%;
  text-align: center;
  border: none;

  font-kerning: none;
  font-feature-settings: 'calt' off;

  display: flex;
  justify-content: center;
  align-items: flex-start;
  cursor: pointer;

  &:focus {
    outline: none !important;
    border: none !important;
    box-shadow: none !important;
  }

  && {
    color: var(--privy-color-foreground);
    /* text-6xl/font-semiBold */
    font-size: 3.75rem;
    font-style: normal;
    font-weight: 600;
    line-height: 5.375rem; /* 143.333% */
  }
`,_=m.span`
  color: var(--privy-color-foreground);
  font-kerning: none;
  font-feature-settings: 'calt' off;

  font-size: 1rem;
  font-style: normal;
  font-weight: 600;
  line-height: 1.5rem; /* 150% */

  margin-top: 0.75rem;
`,fe=({currency:t="usd",value:a,onChange:l,inputMode:p="decimal",autoFocus:h})=>{let[u,o]=g.useState("0"),d=g.useRef(null),c=a??u,j=de[t.toLowerCase()]||"$",C=g.useCallback(r=>{let y=r.target.value,s=(y=y.replace(/[^\d.]/g,"")).split(".");s.length>2&&(y=s[0]+"."+s.slice(1).join("")),s.length===2&&s[1].length>2&&(y=`${s[0]}.${s[1].slice(0,2)}`),y.length>1&&y[0]==="0"&&y[1]!=="."&&(y=y.slice(1)),(y===""||y===".")&&(y="0"),l?l(y):o(y)},[l]),n=g.useCallback(r=>{[46,8,9,27,13,110,190].indexOf(r.keyCode)===-1&&(r.keyCode!==65||r.ctrlKey!==!0)&&(r.keyCode!==67||r.ctrlKey!==!0)&&(r.keyCode!==86||r.ctrlKey!==!0)&&(r.keyCode!==88||r.ctrlKey!==!0)&&(!(r.keyCode>=35)||!(r.keyCode<=39))&&(r.shiftKey||r.keyCode<48||r.keyCode>57)&&(r.keyCode<96||r.keyCode>105)&&r.preventDefault()},[]),b=g.useMemo(()=>(c.includes("."),c),[c]);return e.jsx(e.Fragment,{children:e.jsxs(pe,{onClick:()=>d.current?.focus(),children:[e.jsx(_,{children:j}),b,e.jsx("input",{ref:d,type:"text",inputMode:p,value:b,onChange:C,onKeyDown:n,autoFocus:h,placeholder:"0",style:{width:1,height:"1rem",opacity:0,alignSelf:"center",fontSize:"1rem"}}),e.jsx(_,{style:{opacity:0},children:j})]})})};const me=({opts:t,isLoading:a,onSelectSource:l})=>e.jsx(k,{showClose:!1,showBack:!0,onBack:()=>l(t.source.selectedAsset),title:"Select currency",children:e.jsx(ye,{children:t.source.assets.map(p=>{let{icon:h,name:u}=L[p];return e.jsx(he,{onClick:()=>l(p),disabled:a,children:e.jsxs(ge,{children:[e.jsx(ve,{children:h}),e.jsxs(ke,{children:[e.jsx(xe,{children:u}),e.jsx(Ce,{children:p.toLocaleUpperCase()})]})]})},p)})})});let ye=m.div`
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  width: 100%;
`,he=m.button`
  border-color: var(--privy-color-border-default);
  border-width: 1px;
  border-radius: var(--privy-border-radius-mdlg);
  border-style: solid;
  display: flex;

  && {
    padding: 0.75rem 1rem;
  }
`,ge=m.div`
  display: flex;
  align-items: center;
  gap: 1rem;
  width: 100%;
`,ve=m.div`
  svg {
    width: 2.25rem;
    height: 2.25rem;
    border-radius: var(--privy-border-radius-full);
    overflow: hidden;
  }
`,ke=m.div`
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 0.125rem;
`,xe=m.span`
  color: var(--privy-color-foreground);
  font-size: 0.875rem;
  font-weight: 600;
  line-height: 1.25rem;
`,Ce=m.span`
  color: var(--privy-color-foreground-3);
  font-size: 0.75rem;
  font-weight: 400;
  line-height: 1.125rem;
`;const be=({onClose:t,onAcceptTerms:a,onSelectAmount:l,onSelectSource:p,onEditSourceAsset:h,opts:u,state:o,email:d,isLoading:c})=>o.status==="select-amount"?e.jsx(ae,{onClose:t,onSelectAmount:l,onEditSourceAsset:h,opts:u,isLoading:c}):o.status==="select-source-asset"?e.jsx(me,{onSelectSource:p,opts:u,isLoading:c}):o.status==="kyc-prompt"?e.jsx(se,{onClose:t,onAcceptTerms:a,opts:u,isLoading:c}):o.status==="kyc-incomplete"?e.jsx(oe,{onClose:t,email:d}):o.status==="kyc-success"?e.jsx(ne,{onClose:t}):o.status==="kyc-error"?e.jsx(re,{onClose:t,reason:o.reason}):o.status==="account-details"?e.jsx(X,{onClose:t,data:o.data}):o.status==="create-customer-error"||o.status==="get-customer-error"?e.jsx(te,{onClose:t}):null,Me={component:()=>{let{user:t}=F(),a=D().data;if(!a?.FundWithBankDepositScreen)throw Error("Missing data");let{onSuccess:l,onFailure:p,opts:h,createOrUpdateCustomer:u,getCustomer:o,getOrCreateVirtualAccount:d}=a.FundWithBankDepositScreen,[c,j]=g.useState(h),[C,n]=g.useState({status:"select-amount"}),[b,r]=g.useState(null),[y,s]=g.useState(!1),w=g.useRef(null),E=g.useCallback(async()=>{let i;s(!0),r(null);try{i=await o({kycRedirectUrl:window.location.origin})}catch(f){if(!f||typeof f!="object"||!("status"in f)||f.status!==404)return n({status:"get-customer-error"}),r(f),void s(!1)}if(!i)try{i=await u({hasAcceptedTerms:!1,kycRedirectUrl:window.location.origin})}catch(f){return n({status:"create-customer-error"}),r(f),void s(!1)}if(!i)return n({status:"create-customer-error"}),r(Error("Unable to create customer")),void s(!1);if(i.status==="not_started"&&i.kyc_url)return n({status:"kyc-prompt",kycUrl:i.kyc_url}),void s(!1);if(i.status==="not_started")return n({status:"get-customer-error"}),r(Error("Unexpected user state")),void s(!1);if(i.status==="rejected")return n({status:"kyc-error",reason:i.rejection_reasons?.[0]?.reason}),r(Error("User KYC rejected.")),void s(!1);if(i.status==="incomplete")return n({status:"kyc-incomplete"}),void s(!1);if(i.status!=="active")return n({status:"get-customer-error"}),r(Error("Unexpected user state")),void s(!1);i.status;try{let f=await d({destination:c.destination,provider:c.provider,source:{asset:c.source.selectedAsset}});n({status:"account-details",data:f})}catch(f){return n({status:"create-customer-error"}),r(f),void s(!1)}},[c]),T=g.useCallback(async()=>{if(r(null),s(!0),C.status!=="kyc-prompt")return r(Error("Unexpected state")),void s(!1);let i=K({location:C.kycUrl});if(await u({hasAcceptedTerms:!0}),!i)return r(Error("Unable to begin kyc flow.")),s(!1),void n({status:"create-customer-error"});w.current=new AbortController;let f=await U({operation:async()=>({done:i.location.origin===window.location.origin,closed:i.closed}),until:({done:v,closed:M})=>v||M,delay:0,interval:100,attempts:18e3,signal:w.current.signal});if(f.status==="aborted")return void i.close();if(f.status==="success"&&f.result.closed)return void s(!1);f.status==="success"&&f.result.done&&i.close();let x=await U({operation:()=>o({}),until:v=>v.status==="active"||v.status==="rejected",delay:0,interval:2e3,attempts:60,signal:w.current.signal});if(x.status!=="aborted"){if(x.status==="max_attempts")return n({status:"kyc-incomplete"}),void s(!1);if(x.status,x.result.status==="rejected")return n({status:"kyc-error",reason:x.result.rejection_reasons?.[0]?.reason}),r(Error("User KYC rejected.")),void s(!1);if(x.result.status!=="active")return n({status:"kyc-incomplete"}),void s(!1);i.closed||i.close(),x.result.status;try{n({status:"kyc-success"});let v=await d({destination:c.destination,provider:c.provider,source:{asset:c.source.selectedAsset}});n({status:"account-details",data:v})}catch(v){n({status:"create-customer-error"}),r(v)}finally{s(!1)}}},[n,r,s,u,d,C,c,w]),z=g.useCallback(i=>{n({status:"select-amount"}),j({...c,source:{...c.source,selectedAsset:i}})},[n,j]),B=g.useCallback(()=>{n({status:"select-source-asset"})},[n]);return e.jsx(be,{onClose:g.useCallback(async()=>{w.current?.abort(),b?p(b):await l()},[b,w]),opts:c,state:C,isLoading:y,email:t.email.address,onAcceptTerms:T,onSelectAmount:E,onSelectSource:z,onEditSourceAsset:B})}};export{Me as FundWithBankDepositScreen,Me as default};
