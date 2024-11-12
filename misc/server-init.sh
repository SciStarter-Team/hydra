#!/bin/bash
sed -i 's/#LLMNR=yes/LLMNR=no/g' /etc/systemd/resolved.conf
sed -i 's/#DNS=/DNS=2600:14c0:6::2#ns1.linode.com 92.123.94.2#ns1.linode.com 2600:14c0:6::3#ns2.linode.com 92.123.94.3#ns2.linode.com 2600:14c0:7::3#ns3.linode.com 92.123.95.3#ns3.linode.com 2600:14c0:7::4#ns4.linode.com 92.123.95.4#ns4.linode.com 2600:14c0:7::2#ns5.linode.com 92.123.95.2#ns5.linode.com/g' /etc/systemd/resolved.conf
systemctl restart systemd-resolved
sed -i 's/# %wheel ALL=(ALL:ALL) NOPASSWD: ALL/%wheel ALL=(ALL:ALL) NOPASSWD: ALL/g' /etc/sudoers
sed -i 's/PermitRootLogin yes/PermitRootLogin no/g' /etc/ssh/sshd_config
sed -i 's/PasswordAuthentication yes/PasswordAuthentication no/g' /etc/ssh/sshd_config
pacman -Syy archlinux-keyring
pacman -Su
pacman -S ntp git base-devel supervisor libspatialite
systemctl enable --now ntpd
systemctl enable --now supervisord
useradd -m -G wheel -U gears
cp -r ~/.ssh /home/gears/
chown -R gears:gears /home/gears/.ssh/
sudo -u gears bash -c '(cd ~ && git clone https://aur.archlinux.org/yay-bin.git && cd yay-bin && makepkg -si && cd ~ && rm -rf yay-bin)'
sudo -u gears yay --save --answerclean All --answerdiff None --cleanafter -S nats-server-bin
cat >/home/gears/nats-server.conf <<EOF
host: IP_ADDRESS_OF_ETH1

jetstream {
  store_dir: /home/gears/nats
  max_file_store: 5gb
}
EOF
mkdir /home/gears/nats
read -p "Press Enter to reboot"
reboot
