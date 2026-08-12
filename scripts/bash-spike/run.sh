set -x
cd "/mnt/d/visual studio projects/os/build/spike/bash-5.2"
./configure --without-bash-malloc >configure.log 2>&1
echo "CONFIGURE_EXIT=$?"
tail -5 configure.log
make -j8 >make.log 2>&1
echo "MAKE_EXIT=$?"
tail -15 make.log
ls -l bash 2>/dev/null && echo "BASH_BINARY_BUILT"
