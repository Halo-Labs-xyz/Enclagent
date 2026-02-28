import{r as y,j as e}from"./launchpad.js";import{F as C}from"./launchpad-ExclamationTriangleIcon-DS9jnCFw.js";import{F as E}from"./launchpad-LockClosedIcon-BqF5SfBd.js";import{dy as F,d6 as R,d4 as U,ev as g,eh as w,du as W,dq as P}from"./launchpad-App-D179iYht.js";import{T as x,k as v,u as j}from"./launchpad-ModalHeader-JjfRejxC-gvXxgADS.js";import{r as A}from"./launchpad-Subtitle-CV-2yKE4-xZAXWUw0.js";import{e as S}from"./launchpad-Title-BnzYV3Is-Bc_Ic5pg.js";const M=P.div`
  && {
    border-width: 4px;
  }

  display: flex;
  justify-content: center;
  align-items: center;
  padding: 1rem;
  aspect-ratio: 1;
  border-style: solid;
  border-color: ${t=>t.$color??"var(--privy-color-accent)"};
  border-radius: 50%;
`,z={component:()=>{let{user:t}=F(),{client:$,walletProxy:u,refreshSessionAndUser:b,closePrivyModal:s}=R(),r=U(),{entropyId:m,entropyIdVerifier:T}=r.data?.recoverWallet,[a,f]=y.useState(!1),[i,I]=y.useState(null),[l,h]=y.useState(null);function n(){if(!a){if(l)return r.data?.setWalletPassword?.onFailure(l),void s();if(!i)return r.data?.setWalletPassword?.onFailure(Error("User exited set recovery flow")),void s()}}r.onUserCloseViaDialogOrKeybindRef.current=n;let k=!(!a&&!i);return e.jsxs(e.Fragment,l?{children:[e.jsx(x,{onClose:n},"header"),e.jsx(M,{$color:"var(--privy-color-error)",style:{alignSelf:"center"},children:e.jsx(C,{height:38,width:38,stroke:"var(--privy-color-error)"})}),e.jsx(S,{style:{marginTop:"0.5rem"},children:"Something went wrong"}),e.jsx(g,{style:{minHeight:"2rem"}}),e.jsx(v,{onClick:()=>h(null),children:"Try again"}),e.jsx(j,{})]}:{children:[e.jsx(x,{onClose:n},"header"),e.jsx(E,{style:{width:"3rem",height:"3rem",alignSelf:"center"}}),e.jsx(S,{style:{marginTop:"0.5rem"},children:"Automatically secure your account"}),e.jsx(A,{style:{marginTop:"1rem"},children:"When you log into a new device, you’ll only need to authenticate to access your account. Never get logged out if you forget your password."}),e.jsx(g,{style:{minHeight:"2rem"}}),e.jsx(v,{loading:a,disabled:k,onClick:()=>async function(){f(!0);try{let o=await $.getAccessToken(),c=w(t,m);if(!o||!u||!c)return;if(!(await u.setRecovery({accessToken:o,entropyId:m,entropyIdVerifier:T,existingRecoveryMethod:c.recoveryMethod,recoveryMethod:"privy"})).entropyId)throw Error("Unable to set recovery on wallet");let d=await b();if(!d)throw Error("Unable to set recovery on wallet");let p=w(d,c.address);if(!p)throw Error("Unabled to set recovery on wallet");I(!!d),setTimeout(()=>{r.data?.setWalletPassword?.onSuccess(p),s()},W)}catch(o){h(o)}finally{f(!1)}}(),children:i?"Success":"Confirm"}),e.jsx(j,{})]})}};export{z as SetAutomaticRecoveryScreen,z as default};
