#!/usr/bin/env python
import sys
import time

from mininet.link import TCLink
from mininet.net import Mininet
from mininet.nodelib import NAT
from mininet.topo import Topo


class RelayTestTopo(Topo):
    def build(self):
        # set up inet switch
        inetSwitch = self.addSwitch('s0')

        # add two hosts, both behind NAT
        for index, kind in enumerate(["alice", "bob"]):
            index += 1

            inetIntf = 'nat%s-eth0' % kind
            localIntf = 'nat%s-eth1' % kind
            localIP = '192.168.%d.1' % index
            localSubnet = '192.168.%d.0/24' % index
            natParams = { 'ip' : '%s/24' % localIP }
            # add NAT to topology
            nat = self.addNode('nat%s' % kind, cls=NAT, subnet=localSubnet,
                               inetIntf=inetIntf, localIntf=localIntf)

            switch = self.addSwitch('s%s' % index)
            # connect NAT to inet and local switches
            self.addLink(nat, inetSwitch, intfName1=inetIntf, cls=TCLink, delay = '70ms')
            self.addLink(nat, switch, intfName1=localIntf, params1=natParams)
            # add host and connect to local switch
            host = self.addHost('h%s' % kind,
                                ip='192.168.%d.100/24' % index,
                                defaultRoute='via %s' % localIP)
            self.addLink(host, switch)

        # add relay host
        host = self.addHost('hrelay', ip='10.0.0.1/24')
        self.addLink(host, inetSwitch, cls=TCLink, delay = '30ms')

        host = self.addHost('hredis', ip='10.0.0.2/24')
        self.addLink(host, inetSwitch)


def tcpHolepunch(mininet: Mininet, hRelay, hClient):
    relay = mininet.getNodeByName('hrelay')
    alice = mininet.getNodeByName('halice')
    bob = mininet.getNodeByName('hbob')
    redis = mininet.getNodeByName('hredis')

    redis.cmdPrint("docker run --rm -p 6739:6739 host redis &")
    time.sleep(1)

    relay.cmdPrint(f"{hRelay} --listen-addr {relay.IP()}")
    alice.cmdPrint(f"{hClient} --mode listen --transport tcp")
    bob.cmdPrint(f"{hClient} --mode dial --transport tcp")


if __name__ == '__main__':
    from mininet.net import Mininet
    from mininet.log import setLogLevel

    setLogLevel( 'info' )

    net = Mininet(topo=RelayTestTopo())

    net.run(tcpHolepunch, net, sys.argv[1], sys.argv[2])
