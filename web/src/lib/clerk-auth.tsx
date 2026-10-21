import { jaJP } from "@clerk/localizations";
import { ClerkProvider, HandleSSOCallback, useAuth, useClerk, useSignIn } from "@clerk/react";
import type { ReactNode } from "react";
import { AuthProvider } from "./auth";

export const SSO_CALLBACK_PATH = "/sso-callback";

// Clerk の状態を画面用の形（AuthState）に変換する
function ClerkBridge({ children }: { children: ReactNode }) {
  const { isLoaded, isSignedIn, getToken } = useAuth();
  const { signIn } = useSignIn();
  const clerk = useClerk();
  return (
    <AuthProvider
      value={{
        status: !isLoaded ? "loading" : isSignedIn ? "signed-in" : "signed-out",
        getToken: () => getToken(),
        // ログイン方法は Google だけ（PropFolio と同じ Clerk だが、ここではメール・パスワードを出さない）
        signInWithGoogle: async () => {
          await signIn.sso({
            strategy: "oauth_google",
            redirectUrl: "/",
            redirectCallbackUrl: SSO_CALLBACK_PATH,
          });
        },
        signOut: () => clerk.signOut({ redirectUrl: "/" }),
      }}
    >
      {children}
    </AuthProvider>
  );
}

export function ClerkAuth({
  publishableKey,
  children,
}: {
  publishableKey: string;
  children: ReactNode;
}) {
  return (
    <ClerkProvider publishableKey={publishableKey} localization={jaJP}>
      <ClerkBridge>
        {window.location.pathname === SSO_CALLBACK_PATH ? <SsoCallback /> : children}
      </ClerkBridge>
    </ClerkProvider>
  );
}

// Google から戻ってきたところ。セッションができたらトップへ
function SsoCallback() {
  const go = (path: string) => () => window.location.replace(path);
  return (
    <HandleSSOCallback
      navigateToApp={go("/")}
      navigateToSignIn={go("/")}
      navigateToSignUp={go("/")}
    />
  );
}
