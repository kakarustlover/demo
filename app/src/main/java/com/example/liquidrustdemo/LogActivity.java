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
import java.io.FileOutputStream;
import java.io.InputStreamReader;
import java.io.RandomAccessFile;
import java.nio.charset.StandardCharsets;
import java.text.SimpleDateFormat;
import java.util.Date;
import java.util.Locale;

public class LogActivity extends Activity {
    private static final int APP_LOG_CHARS = 150_000;
    private static final int PREV_LOG_CHARS = 50_000;
    private static final int LOGCAT_CHARS = 100_000;
            private static final String[] TIER_NAMES = {"ULTRA", "HIGH", "OPTIMIZED", "LITE", "MINI", "AURORA UI"};
    private static final String[] MODE_LABELS = {"ULTRA", "OPTIMIZED", "AURORA UI"};
    private static final int[] MODE_TIER = {0, 2, 5};

    private TextView logView;
    private TextView qualityView;
    private Button autoButton;
    private Button animButton;
    private ScrollView scroll;
    private volatile String lastText = "";

    // written to config.txt before every demo start
    private int tier = 2;
    private boolean auto = true;
    private boolean anim = false;

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
        title.setText("Liquid Rust Demo v2 - Log viewer");
        title.setTextColor(0xFFFFFFFF);
        title.setTextSize(17);
        title.setTypeface(Typeface.DEFAULT_BOLD);
        title.setPadding(dp(14), dp(12), dp(14), dp(4));
        root.addView(title);

        qualityView = new TextView(this);
        qualityView.setTextColor(0xFFFFE066);
        qualityView.setTextSize(12);
        qualityView.setPadding(dp(14), 0, dp(14), dp(6));
        root.addView(qualityView);

                LinearLayout rowQ1 = row();
        for (int i = 0; i < MODE_LABELS.length; i++) {
            final int idx = MODE_TIER[i];
            rowQ1.addView(button(MODE_LABELS[i], 0xFF3A4A6B, v -> setTier(idx)));
        }
        root.addView(rowQ1);
        

        LinearLayout rowA = row();
        autoButton = button("AUTO", 0xFF4A3A6B, v -> {
            auto = !auto;
            updateLabel();
        });
        animButton = button("BG ANIM", 0xFF4A3A6B, v -> {
            anim = !anim;
            updateLabel();
        });
        rowA.addView(autoButton);
        rowA.addView(animButton);
        rowA.addView(button("BENCH", 0xFF8A5A1E, v -> startDemo(true)));
        root.addView(rowA);

        LinearLayout row1 = row();
        row1.addView(button("START DEMO", 0xFF1E7F4F, v -> startDemo(false)));
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
        updateLabel();
    }

    @Override
    protected void onResume() {
        super.onResume();
        updateLabel();
        refresh();
    }

    /** User picked a quality: also resets the automatic fallback memory. */
    private void setTier(int t) {
        tier = t;
        new File(getFilesDir(), "state.txt").delete();
        new File(getFilesDir(), "running.flag").delete();
        updateLabel();
        Toast.makeText(this, TIER_NAMES[t] + " selected (fallback memory reset)", Toast.LENGTH_SHORT).show();
    }

    private void updateLabel() {
        String state = readTail(new File(getFilesDir(), "state.txt"), 200).trim().replace('\n', ' ');
        if (state.startsWith("(")) state = "none";
        qualityView.setText("Tier: " + TIER_NAMES[tier] + " | auto " + (auto ? "ON" : "OFF")
                + " | bg anim " + (anim ? "ON" : "OFF") + "\nfallback memory: " + state);
        autoButton.setText("AUTO: " + (auto ? "ON" : "OFF"));
        animButton.setText("ANIM: " + (anim ? "ON" : "OFF"));
    }

    private void writeCfg(boolean bench) {
        String s = "tier=" + tier + "\nauto=" + (auto ? 1 : 0) + "\nanim=" + (anim ? 1 : 0)
                + "\nbench=" + (bench ? 1 : 0) + "\n";
        try (FileOutputStream out = new FileOutputStream(new File(getFilesDir(), "config.txt"))) {
            out.write(s.getBytes(StandardCharsets.UTF_8));
        } catch (Exception e) {
            Toast.makeText(this, "Cannot save: " + e, Toast.LENGTH_LONG).show();
        }
    }

    private void startDemo(boolean bench) {
        writeCfg(bench);
        if (bench) {
            Toast.makeText(this, "Benchmark: 6 tiers x 5 s. Then Back and COPY ALL", Toast.LENGTH_LONG).show();
        }
        startActivity(new Intent(this, NativeActivity.class));
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
        b.setTextSize(11);
        b.setBackgroundColor(color);
        b.setOnClickListener(l);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(0, dp(44), 1f);
        lp.setMargins(dp(4), dp(3), dp(4), dp(3));
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
                scroll.post(() -> scroll.fullScroll(View.FOCUS_UP));
            });
        }).start();
    }

    /** Key lines only: errors, tier changes, crash detection, benchmark, last STAT. */
    private static String summarize(String log) {
        StringBuilder sb = new StringBuilder();
        String lastStat = null;
        for (String line : log.split("\n")) {
            if (line.contains(": STAT ")) {
                lastStat = line;
            } else if (line.contains("TIER ") || line.contains("ERROR") || line.contains("PANIC")
                    || line.contains("PREVIOUS RUN") || line.contains("BENCH") || line.contains("FALLBACK")
                    || line.contains("HEALTH") || line.contains("suspended before")) {
                sb.append(line).append('\n');
            }
        }
        if (lastStat != null) sb.append(lastStat).append('\n');
        return sb.toString();
    }

    private String buildReport() {
        StringBuilder sb = new StringBuilder();
        File dir = getFilesDir();
        String cur = readTail(new File(dir, "liquid_demo.log"), APP_LOG_CHARS);
        String prev = readTail(new File(dir, "liquid_demo.prev.log"), PREV_LOG_CHARS);

        sb.append("=== DEVICE ===\n");
        sb.append("Time: ").append(new SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.US).format(new Date())).append('\n');
        sb.append("Model: ").append(Build.MANUFACTURER).append(' ').append(Build.MODEL).append('\n');
        sb.append("Android: ").append(Build.VERSION.RELEASE).append(" (SDK ").append(Build.VERSION.SDK_INT).append(")\n");
        sb.append("ABI: ").append(Build.SUPPORTED_ABIS.length > 0 ? Build.SUPPORTED_ABIS[0] : "?").append('\n');
        sb.append("Density: ").append(getResources().getDisplayMetrics().density).append('\n');
        sb.append("config.txt: ").append(readTail(new File(dir, "config.txt"), 200).trim().replace('\n', ' ')).append('\n');
        sb.append("state.txt: ").append(readTail(new File(dir, "state.txt"), 200).trim().replace('\n', ' ')).append('\n');
        sb.append("running.flag: ").append(new File(dir, "running.flag").exists() ? "PRESENT (last run did not end cleanly)" : "absent").append('\n');

        sb.append("\n=== SUMMARY (this run) ===\n").append(summarize(cur));
        sb.append("\n=== SUMMARY (previous run) ===\n").append(summarize(prev));
        sb.append("\n=== APP LOG (this run) ===\n").append(cur);
        sb.append("\n=== APP LOG (previous run) ===\n").append(prev);
        sb.append("\n=== LOGCAT (this app, newest last) ===\n").append(readLogcat(LOGCAT_CHARS));
        return sb.toString();
    }

    private static String readTail(File f, int maxChars) {
        if (!f.exists()) return "(no file yet)\n";
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
        if (t.length() > 150_000) t = t.substring(0, 150_000);
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
