import * as cdk from 'aws-cdk-lib';
import * as dsql from 'aws-cdk-lib/aws-dsql';
import { Construct } from 'constructs';

export class Ch02Stack extends cdk.Stack {
  constructor(scope: Construct, id: string, props?: cdk.StackProps) {
    super(scope, id, props);

    const cluster = new dsql.CfnCluster(this, 'DsqlCluster', {
      deletionProtectionEnabled: false,
      tags: [{
        key: 'Name',
        value: 'ch02'
      }]
    });

    new cdk.CfnOutput(this, 'ClusterId', {
      value: cluster.attrIdentifier,
      description: 'Aurora DSQL Cluster ID'
    });
  }
}
