import{r as m,j as e}from"./launchpad.js";import{dq as r}from"./launchpad-App-D179iYht.js";import{$ as p}from"./launchpad-ModalHeader-JjfRejxC-gvXxgADS.js";import{e as f}from"./launchpad-ErrorMessage-D8VaAP5m-xTbV0e-V.js";import{r as x}from"./launchpad-LabelXs-oqZNqbm_-D2WTgiPX.js";import{d as h}from"./launchpad-Address-D-q_5it9-DVYLY2Ks.js";import{d as j}from"./launchpad-shared-FM0rljBt-BThDrsH1.js";import{C as g}from"./launchpad-check-C5kCRahI.js";import{C as u}from"./launchpad-copy-DYa4Op8P.js";let v=r(j)`
  && {
    padding: 0.75rem;
    height: 56px;
  }
`,y=r.div`
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
`,C=r.div`
  display: flex;
  flex-direction: column;
  gap: 0;
`,z=r.div`
  font-size: 12px;
  line-height: 1rem;
  color: var(--privy-color-foreground-3);
`,b=r(x)`
  text-align: left;
  margin-bottom: 0.5rem;
`,w=r(f)`
  margin-top: 0.25rem;
`,E=r(p)`
  && {
    gap: 0.375rem;
    font-size: 14px;
  }
`;const P=({errMsg:t,balance:s,address:a,className:c,title:n,showCopyButton:d=!1})=>{let[o,l]=m.useState(!1);return m.useEffect(()=>{if(o){let i=setTimeout(()=>l(!1),3e3);return()=>clearTimeout(i)}},[o]),e.jsxs("div",{children:[n&&e.jsx(b,{children:n}),e.jsx(v,{className:c,$state:t?"error":void 0,children:e.jsxs(y,{children:[e.jsxs(C,{children:[e.jsx(h,{address:a,showCopyIcon:!1}),s!==void 0&&e.jsx(z,{children:s})]}),d&&e.jsx(E,{onClick:function(i){i.stopPropagation(),navigator.clipboard.writeText(a).then(()=>l(!0)).catch(console.error)},size:"sm",children:e.jsxs(e.Fragment,o?{children:["Copied",e.jsx(g,{size:14})]}:{children:["Copy",e.jsx(u,{size:14})]})})]})}),t&&e.jsx(w,{children:t})]})};export{P as j};
