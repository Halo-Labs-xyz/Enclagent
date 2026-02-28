import{r as h,j as e}from"./launchpad.js";import{dq as a,dy as b,d6 as E,d8 as v,d9 as k,d_ as C,eS as I}from"./launchpad-App-D179iYht.js";import{a as P,c as x}from"./launchpad-TodoList-CgrU7uwu-SiSPC2Wo.js";import{n as L}from"./launchpad-ScreenLayout-Ca6ml9wY-CeAdm0Km.js";import{C as S}from"./launchpad-circle-check-big-Bg8vqBbP.js";import{F as w}from"./launchpad-fingerprint-pattern-eZF4302v.js";import{c as N}from"./launchpad-createLucideIcon-BSOjrqRH.js";import"./launchpad-check-C5kCRahI.js";import"./launchpad-ModalHeader-JjfRejxC-gvXxgADS.js";import"./launchpad-Screen-DE3ldE_X-BBf8NvvX.js";import"./launchpad-index-Dq_xe9dz-bxbF5Mdc.js";/**
 * @license lucide-react v0.554.0 - ISC
 *
 * This source code is licensed under the ISC license.
 * See the LICENSE file in the root directory of this source tree.
 */const A=[["path",{d:"M10 11v6",key:"nco0om"}],["path",{d:"M14 11v6",key:"outv1u"}],["path",{d:"M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6",key:"miytrc"}],["path",{d:"M3 6h18",key:"d0wm0j"}],["path",{d:"M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2",key:"e791ji"}]],W=N("trash-2",A),_=({passkeys:o,isLoading:d,errorReason:u,success:y,expanded:t,onLinkPasskey:l,onUnlinkPasskey:s,onExpand:n,onBack:r,onClose:i})=>e.jsx(L,y?{title:"Passkeys updated",icon:S,iconVariant:"success",primaryCta:{label:"Done",onClick:i},onClose:i,watermark:!0}:t?{icon:w,title:"Your passkeys",onBack:r,onClose:i,watermark:!0,children:e.jsx(j,{passkeys:o,expanded:t,onUnlink:s,onExpand:n})}:{icon:w,title:"Set up passkey verification",subtitle:"Verify with passkey",primaryCta:{label:"Add new passkey",onClick:l,loading:d},onClose:i,watermark:!0,helpText:u||void 0,children:o.length===0?e.jsx(B,{}):e.jsx($,{children:e.jsx(j,{passkeys:o,expanded:t,onUnlink:s,onExpand:n})})});let $=a.div`
  margin-bottom: 12px;
`,j=({passkeys:o,expanded:d,onUnlink:u,onExpand:y})=>{let[t,l]=h.useState([]),s=d?o.length:2;return e.jsxs("div",{children:[e.jsx(T,{children:"Your passkeys"}),e.jsxs(z,{children:[o.slice(0,s).map(n=>{return e.jsxs(D,{children:[e.jsxs("div",{children:[e.jsx(V,{children:(r=n,r.authenticatorName?r.createdWithBrowser?`${r.authenticatorName} on ${r.createdWithBrowser}`:r.authenticatorName:r.createdWithBrowser?r.createdWithOs?`${r.createdWithBrowser} on ${r.createdWithOs}`:`${r.createdWithBrowser}`:"Unknown device")}),e.jsxs(O,{children:["Last used:"," ",(n.latestVerifiedAt??n.firstVerifiedAt)?.toLocaleString()??"N/A"]})]}),e.jsx(R,{disabled:t.includes(n.credentialId),onClick:()=>(async i=>{l(p=>p.concat([i])),await u(i),l(p=>p.filter(m=>m!==i))})(n.credentialId),children:t.includes(n.credentialId)?e.jsx(I,{}):e.jsx(W,{size:16})})]},n.credentialId);var r}),o.length>2&&!d&&e.jsx(U,{onClick:y,children:"View all"})]})]})},B=()=>e.jsxs(P,{style:{color:"var(--privy-color-foreground)"},children:[e.jsx(x,{children:"Verify with Touch ID, Face ID, PIN, or hardware key"}),e.jsx(x,{children:"Takes seconds to set up and use"}),e.jsx(x,{children:"Use your passkey to verify transactions and login to your account"})]});const ne={component:()=>{let{user:o,unlinkPasskey:d}=b(),{linkWithPasskey:u,closePrivyModal:y}=E(),t=o?.linkedAccounts.filter(c=>c.type==="passkey"),[l,s]=h.useState(!1),[n,r]=h.useState(""),[i,p]=h.useState(!1),[m,f]=h.useState(!1);return h.useEffect(()=>{t.length===0&&f(!1)},[t.length]),e.jsx(_,{passkeys:t,isLoading:l,errorReason:n,success:i,expanded:m,onLinkPasskey:()=>{s(!0),u().then(()=>p(!0)).catch(c=>{if(c instanceof v){if(c.privyErrorCode===k.CANNOT_LINK_MORE_OF_TYPE)return void r("Cannot link more passkeys to account.");if(c.privyErrorCode===k.PASSKEY_NOT_ALLOWED)return void r("Passkey request timed out or rejected by user.")}r("Unknown error occurred.")}).finally(()=>{s(!1)})},onUnlinkPasskey:async c=>(s(!0),await d(c).then(()=>p(!0)).catch(g=>{g instanceof v&&g.privyErrorCode===k.MISSING_MFA_CREDENTIALS?r("Cannot unlink a passkey enrolled in MFA"):r("Unknown error occurred.")}).finally(()=>{s(!1)})),onExpand:()=>f(!0),onBack:()=>f(!1),onClose:()=>y()})}},te=a.div`
  display: flex;
  align-items: center;
  justify-content: center;
  width: 180px;
  height: 90px;
  border-radius: 50%;
  svg + svg {
    margin-left: 12px;
  }
  > svg {
    z-index: 2;
    color: var(--privy-color-accent) !important;
    stroke: var(--privy-color-accent) !important;
    fill: var(--privy-color-accent) !important;
  }
`;let M=C`
  && {
    width: 100%;
    font-size: 0.875rem;
    line-height: 1rem;

    /* Tablet and Up */
    @media (min-width: 440px) {
      font-size: 14px;
    }

    display: flex;
    gap: 12px;
    justify-content: center;

    padding: 6px 8px;
    background-color: var(--privy-color-background);
    transition: background-color 200ms ease;
    color: var(--privy-color-accent) !important;

    :focus {
      outline: none;
      box-shadow: none;
    }
  }
`;const U=a.button`
  ${M}
`;let z=a.div`
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 0.8rem;
  padding: 0.5rem 0rem 0rem;
  flex-grow: 1;
  width: 100%;
`,T=a.div`
  line-height: 20px;
  height: 20px;
  font-size: 1em;
  font-weight: 450;
  display: flex;
  justify-content: flex-beginning;
  width: 100%;
`,V=a.div`
  font-size: 1em;
  line-height: 1.3em;
  font-weight: 500;
  color: var(--privy-color-foreground-2);
  padding: 0.2em 0;
`,O=a.div`
  font-size: 0.875rem;
  line-height: 1rem;
  color: #64668b;
  padding: 0.2em 0;
`,D=a.div`
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 1em;
  gap: 10px;
  font-size: 0.875rem;
  line-height: 1rem;
  text-align: left;
  border-radius: 8px;
  border: 1px solid #e2e3f0 !important;
  width: 100%;
  height: 5em;
`,F=C`
  :focus,
  :hover,
  :active {
    outline: none;
  }
  display: flex;
  width: 2em;
  height: 2em;
  justify-content: center;
  align-items: center;
  svg {
    color: var(--privy-color-error);
  }
  svg:hover {
    color: var(--privy-color-foreground-3);
  }
`,R=a.button`
  ${F}
`;export{te as DoubleIconWrapper,U as LinkButton,ne as LinkPasskeyScreen,_ as LinkPasskeyView,ne as default};
