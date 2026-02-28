import{j as n}from"./launchpad.js";import{bY as j,de as $,dq as l}from"./launchpad-App-CJN8YHj9.js";import{m as g,l as a,o as d,c as p}from"./launchpad-ethers-VWEScvge-CDamq4Ed.js";import{C as k}from"./launchpad-getFormattedUsdFromLamports-B6EqSEho-C-HCdwKa.js";import{t as y}from"./launchpad-transaction-CnfuREWo-BX0tihxo.js";const P=({weiQuantities:e,tokenPrice:r,tokenSymbol:o})=>{let t=a(e),i=r?d(t,r):void 0,s=p(t,o);return n.jsx(c,{children:i||s})},q=({weiQuantities:e,tokenPrice:r,tokenSymbol:o})=>{let t=a(e),i=r?d(t,r):void 0,s=p(t,o);return n.jsx(c,{children:i?n.jsxs(n.Fragment,{children:[n.jsx(S,{children:"USD"}),i==="<$0.01"?n.jsxs(m,{children:[n.jsx(h,{children:"<"}),"$0.01"]}):i]}):s})},D=({quantities:e,tokenPrice:r,tokenSymbol:o="SOL",tokenDecimals:t=9})=>{let i=e.reduce((f,u)=>f+u,0n),s=r&&o==="SOL"&&t===9?k(i,r):void 0,x=o==="SOL"&&t===9?y(i):`${j(i,t)} ${o}`;return n.jsx(c,{children:s?n.jsx(n.Fragment,{children:s==="<$0.01"?n.jsxs(m,{children:[n.jsx(h,{children:"<"}),"$0.01"]}):s}):x})};let c=l.span`
  font-size: 14px;
  line-height: 140%;
  display: flex;
  gap: 4px;
  align-items: center;
`,S=l.span`
  font-size: 12px;
  line-height: 12px;
  color: var(--privy-color-foreground-3);
`,h=l.span`
  font-size: 10px;
`,m=l.span`
  display: flex;
  align-items: center;
`;function v(e,r){return`https://explorer.solana.com/account/${e}?chain=${r}`}const F=e=>n.jsx(b,{href:e.chainType==="ethereum"?g(e.chainId,e.walletAddress):v(e.walletAddress,e.chainId),target:"_blank",children:$(e.walletAddress)});let b=l.a`
  &:hover {
    text-decoration: underline;
  }
`;export{D as f,q as h,P as p,F as v};
