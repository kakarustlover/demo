package com.example.liquidrustdemo;

import android.app.Activity;
import android.app.NativeActivity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.graphics.Typeface;
import android.os.Build;
import android.os.Bundle;
import android.view.View;
import android.view.ViewGroup;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import android.widget.Toast;

import java.io.BufferedReader;
import java.io.File;
import java.io.InputStreamReader;
import java.io.RandomAccessFile;
import java.nio.charset.StandardCharsets;
import java.text.SimpleDateFormat;
import java.util.Date;
import java.util.Locale;

public class LogActivity extends Activity {
    private static final int APP_LOG_CHARS = 150_000;
    private static final int PREV_LOG_CHARS = 50_000;
    private static final int LOGCAT_CHARS = 120_000;

    private TextView logView;
    private ScrollView scroll;
    private volatile String lastText = "";

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(0xFF101014);
        root.setOnApplyWindowInsetsListener((v, in) -> {
            v.setPadding(in.getSystemWindowInsetLeft(), in.getSystemWindowInsetTop(),
                    in.getSystemWindowInsetRight(), in.getSystemWindowInsetBottom());
            return in;
        });

        TextView title = new TextView(this);
        title.setText("Liquid Rust Demo - Log viewer");
        title.setTextColor(0xFFFFFFFF);
        title.setTextSize(17);
        title.setTypeface(Typeface.DEFAULT_BOLD);
        title.setPadding(dp(14), dp(12), dp(14), dp(4));
        root.addView(title);

        TextView hint = new TextView(this);
        hint.setText("Tap START DEMO. If the screen stays black, press Back, then REFRESH and COPY ALL.");
        hint.setTextColor(0xFF9A9AA8);
        hint.setTextSize(12);
        hint.setPadding(dp(14), 0, dp(14), dp(8));
        root.addView(hint);

        LinearLayout row1 = row();
        row1.addView(button("START DEMO", 0xFF1E7F4F, v ->
                startActivity(new Intent(this, NativeActivity.class))));
        row1.addView(button("REFRESH", 0xFF2D3A55, v -> refresh()));
        root.addView(row1);

        LinearLayout row2 = row();
        row2.addView(button("COPY ALL", 0xFF2D3A55, v -> copyAll()));
        row2.addView(button("SHARE", 0xFF2D3A55, v -> share()));
        row2.addView(button("CLEAR", 0xFF553030, v -> clearLogs()));
        root.addView(row2);

        scroll = new ScrollView(this);
        logView = new TextView(this);
        logView.setTypeface(Typeface.MONOSPACE);
        logView.setTextSize(10.5f);
        logView.setTextColor(0xFFD0D0D8);
        logView.setTextIsSelectable(true);
        logView.setPadding(dp(8), dp(8), dp(8), dp(8));
        scroll.addView(logView, new ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        root.addView(scroll, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f));

        setContentView(root);
    }

    @Override
    protected void onResume() {
        super.onResume();
        refresh();
    }

    private int dp(int v) {
        return Math.round(v * getResources().getDisplayMetrics().density);
    }

    private LinearLayout row() {
        LinearLayout r = new LinearLayout(this);
        r.setOrientation(LinearLayout.HORIZONTAL);
        r.setPadding(dp(8), 0, dp(8), 0);
        return r;
    }

    private Button button(String text, int color, View.OnClickListener l) {
        Button b = new Button(this);
        b.setText(text);
        b.setTextColor(0xFFFFFFFF);
        b.setTextSize(12);
        b.setBackgroundColor(color);
        b.setOnClickListener(l);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(0, dp(46), 1f);
        lp.setMargins(dp(4), dp(4), dp(4), dp(4));
        b.setLayoutParams(lp);
        return b;
    }

    private void refresh() {
        logView.setText("Loading...");
        new Thread(() -> {
            final String text = buildReport();
            lastText = text;
            runOnUiThread(() -> {
                logView.setText(text);
                scroll.post(() -> scroll.fullScroll(View.FOCUS_DOWN));
            });
        }).start();
    }

    private String buildReport() {
        StringBuilder sb = new StringBuilder();
        sb.append("=== DEVICE ===\n");
        sb.append("Time: ").append(new SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.US).format(new Date())).append('\n');
        sb.append("Model: ").append(Build.MANUFACTURER).append(' ').append(Build.MODEL).append('\n');
        sb.append("Android: ").append(Build.VERSION.RELEASE).append(" (SDK ").append(Build.VERSION.SDK_INT).append(")\n");
        sb.append("ABI: ").append(Build.SUPPORTED_ABIS.length > 0 ? Build.SUPPORTED_ABIS[0] : "?").append('\n');
        sb.append("Density: ").append(getResources().getDisplayMetrics().density).append('\n');

        File dir = getFilesDir();
        sb.append("\n=== APP LOG (this run) ===\n");
        sb.append(readTail(new File(dir, "liquid_demo.log"), APP_LOG_CHARS));
        sb.append("\n=== APP LOG (previous run) ===\n");
        sb.append(readTail(new File(dir, "liquid_demo.prev.log"), PREV_LOG_CHARS));
        sb.append("\n=== LOGCAT (this app, newest last) ===\n");
        sb.append(readLogcat(LOGCAT_CHARS));
        return sb.toString();
    }

    private static String readTail(File f, int maxChars) {
        if (!f.exists()) return "(no file yet: the demo has not been started)\n";
        try (RandomAccessFile raf = new RandomAccessFile(f, "r")) {
            long len = raf.length();
            long start = Math.max(0, len - (long) maxChars * 2);
            raf.seek(start);
            byte[] buf = new byte[(int) (len - start)];
            raf.readFully(buf);
            String s = new String(buf, StandardCharsets.UTF_8);
            if (start > 0) {
                int nl = s.indexOf('\n');
                if (nl >= 0) s = s.substring(nl + 1);
                s = "...(older lines cut)...\n" + s;
            }
            if (s.length() > maxChars) s = s.substring(s.length() - maxChars);
            return s;
        } catch (Exception e) {
            return "(cannot read " + f.getName() + ": " + e + ")\n";
        }
    }

    private static String readLogcat(int maxChars) {
        try {
            Process p = new ProcessBuilder("logcat", "-d", "-v", "threadtime", "-t", "2500")
                    .redirectErrorStream(true).start();
            StringBuilder sb = new StringBuilder();
            try (BufferedReader r = new BufferedReader(new InputStreamReader(p.getInputStream()))) {
                String line;
                while ((line = r.readLine()) != null) sb.append(line).append('\n');
            }
            String s = sb.toString();
            if (s.length() > maxChars) s = "...(older lines cut)...\n" + s.substring(s.length() - maxChars);
            return s.isEmpty() ? "(logcat empty)\n" : s;
        } catch (Exception e) {
            return "(logcat not available: " + e + ")\n";
        }
    }

    private void copyAll() {
        ClipboardManager cm = (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
        cm.setPrimaryClip(ClipData.newPlainText("liquid-rust-demo-log", lastText));
        Toast.makeText(this, "Copied " + lastText.length() + " characters", Toast.LENGTH_SHORT).show();
    }

    private void share() {
        String t = lastText;
        if (t.length() > 150_000) t = t.substring(t.length() - 150_000);
        Intent i = new Intent(Intent.ACTION_SEND);
        i.setType("text/plain");
        i.putExtra(Intent.EXTRA_SUBJECT, "Liquid Rust Demo log");
        i.putExtra(Intent.EXTRA_TEXT, t);
        startActivity(Intent.createChooser(i, "Share log"));
    }

    private void clearLogs() {
        File dir = getFilesDir();
        new File(dir, "liquid_demo.log").delete();
        new File(dir, "liquid_demo.prev.log").delete();
        try {
            new ProcessBuilder("logcat", "-c").start().waitFor();
        } catch (Exception ignored) {
        }
        refresh();
    }
}
