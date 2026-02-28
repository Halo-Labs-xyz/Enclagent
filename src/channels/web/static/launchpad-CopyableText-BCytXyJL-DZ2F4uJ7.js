import{r as p,j as e}from"./launchpad.js";import{dq as n}from"./launchpad-App-CJN8YHj9.js";import{C as x}from"./launchpad-check-C5kCRahI.js";import{C as u}from"./launchpad-copy-DYa4Op8P.js";let a=n.button`
  display: flex;
  align-items: center;
  justify-content: end;
  gap: 0.5rem;

  svg {
    width: 0.875rem;
    height: 0.875rem;
  }
`,d=n.span`
  display: flex;
  align-items: center;
  gap: 0.25rem;
  font-size: 0.875rem;
  color: var(--privy-color-foreground-2);
`,h=n(x)`
  color: var(--privy-color-icon-success);
  flex-shrink: 0;
`,m=n(u)`
  color: var(--privy-color-icon-muted);
  flex-shrink: 0;
`;function y({children:r,iconOnly:l,value:o,hideCopyIcon:i,...c}){let[s,t]=p.useState(!1);return e.jsxs(a,{...c,onClick:()=>{navigator.clipboard.writeText(o||(typeof r=="string"?r:"")).catch(console.error),t(!0),setTimeout(()=>t(!1),1500)},children:[r," ",s?e.jsxs(d,{children:[e.jsx(h,{})," ",!l&&"Copied"]}):!i&&e.jsx(m,{})]})}const C=({value:r,includeChildren:l,children:o,...i})=>{let[c,s]=p.useState(!1),t=()=>{navigator.clipboard.writeText(r).catch(console.error),s(!0),setTimeout(()=>s(!1),1500)};return e.jsxs(e.Fragment,{children:[l?e.jsx(a,{...i,onClick:t,children:o}):e.jsx(e.Fragment,{children:o}),e.jsx(a,{...i,onClick:t,children:c?e.jsx(d,{children:e.jsx(h,{})}):e.jsx(m,{})})]})};export{y as m,C as p};
