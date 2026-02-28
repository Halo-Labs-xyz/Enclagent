/**
 * Entry point for esbuild bundle. Exports Privy + React for launchpad.
 * Run: node scripts/bundle-privy.mjs
 */
import React from "react";
import { createRoot } from "react-dom/client";
import {
  PrivyProvider,
  useLogin,
  usePrivy,
  useWallets,
} from "@privy-io/react-auth";

export { React, createRoot, PrivyProvider, useLogin, usePrivy, useWallets };
