# IIS Reverse Proxy

1. IIS URL RewriteとApplication Request Routingを導入します。
2. ARRのProxyを有効にします。
3. HTTPS Siteの物理パスへweb.configを配置します。
4. IIS Siteからhttp://127.0.0.1:8787/が取得できることを確認します。
5. WakeBridgeのSecure Cookieを有効にします。

既存Siteのbinding、証明書、認証、URL Rewrite規則を上書きする前にバックアップしてください。このファイルはWakeBridge用の最小Reverse Proxy規則です。
