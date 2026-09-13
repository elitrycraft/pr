package id.or.oo.pr.engine;

import java.io.File;

public interface ProotHost {
    File getPrefixDir();
    File getHomeDir();
    String getPackageName();
    File getCacheDir();
}
